use std::{hash::Hash, sync::LazyLock};

use async_trait::async_trait;
use regex::Regex;
use rspack_cacheable::with::AsVecConverter;
use rspack_collections::Identifiable;
use rspack_core::{
  rspack_sources::{ConcatSource, RawStringSource, SourceExt},
  to_comment_with_nl, ApplyContext, BoxModule, BuildMetaExportsType, ChunkGraph,
  ChunkInitFragments, ChunkUkey, Compilation, CompilationParams, CompilerCompilation,
  CompilerOptions, ExportInfo, ExportInfoGetter, ExportProvided, ExportsInfo, Module, ModuleGraph,
  ModuleIdentifier, Plugin, PluginContext, UsageState,
};
use rspack_error::Result;
use rspack_hash::RspackHash;
use rspack_hook::{plugin, plugin_hook};
use rspack_plugin_css::{
  plugin::{CssModulesRenderModulePackage, CssModulesRenderSource},
  CssPlugin,
};
use rspack_plugin_javascript::{
  JavascriptModulesChunkHash, JavascriptModulesRenderModulePackage, JsPlugin, RenderSource,
};
use rustc_hash::FxHashSet;

static COMMENT_END_REGEX: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"\*/").expect("should init regex"));

#[plugin]
#[derive(Debug, Default)]
pub struct ModuleInfoHeaderPlugin {
  verbose: bool,
}

fn print_exports_info_to_source<F>(
  source: &mut ConcatSource,
  ident: &str,
  exports_info_id: ExportsInfo,
  request_shortener: &F,
  already_printed: &mut FxHashSet<ExportInfo>,
  module_graph: &ModuleGraph,
) where
  F: Fn(&ModuleIdentifier) -> String,
{
  let other_exports_info = exports_info_id.other_exports_info(module_graph);

  let mut already_printed_exports = 0;

  let mut printed_exports = vec![];

  for export_info in exports_info_id.ordered_exports(module_graph) {
    if !already_printed.contains(&export_info) {
      already_printed.insert(export_info);
      printed_exports.push(export_info);
    } else {
      already_printed_exports += 1;
    }
  }
  let mut show_other_exports = false;
  if !already_printed.contains(&other_exports_info) {
    already_printed.insert(other_exports_info);
    show_other_exports = true;
  } else {
    already_printed_exports += 1;
  }

  // print the exports
  for export_info in &printed_exports {
    let info = export_info.as_data(module_graph);

    let export_name: String = ExportInfoGetter::name(info)
      .map(|n| n.to_string())
      .unwrap_or("null".into());
    let provide_info = ExportInfoGetter::get_provided_info(info);
    let usage_info = ExportInfoGetter::get_used_info(info);
    let rename_info = ExportInfoGetter::get_rename_info(info);

    let target_desc = match export_info.get_target(module_graph) {
      Some(resolve_target) => {
        let target_module = request_shortener(&resolve_target.module);
        match resolve_target.export {
          None => format!("-> {target_module}"),
          Some(es) => {
            let exp = es.iter().map(|a| a.as_str()).collect::<Vec<_>>().join(".");
            format!(" -> {target_module} {exp}")
          }
        }
      }
      None => "".into(),
    };

    let export_str = format!(
      r#"{ident}export {export_name} [{provide_info}] [{usage_info}] [{rename_info}]{target_desc}"#,
    );

    source.add(RawStringSource::from(to_comment_with_nl(&export_str)));

    if let Some(exports_info) = &ExportInfoGetter::exports_info(info) {
      print_exports_info_to_source(
        source,
        &format!("{ident}  "),
        *exports_info,
        request_shortener,
        already_printed,
        module_graph,
      );
    }
  }

  if already_printed_exports > 0 {
    source.add(RawStringSource::from(to_comment_with_nl(&format!(
      "{ident}... {already_printed_exports} already listed exports",
    ))));
  }

  if show_other_exports {
    let other_exports_info = exports_info_id.other_exports_info(module_graph);
    let other_exports_info_data = other_exports_info.as_data(module_graph);

    let target = other_exports_info.get_target(module_graph);

    if target.is_some()
      || !matches!(
        ExportInfoGetter::provided(other_exports_info_data),
        Some(ExportProvided::NotProvided)
      )
      || ExportInfoGetter::get_used(other_exports_info_data, None) != UsageState::Unused
    {
      let title = if !printed_exports.is_empty() || already_printed_exports > 0 {
        "other exports"
      } else {
        "exports"
      };

      let provide_info = ExportInfoGetter::get_provided_info(other_exports_info_data);
      let used_info = ExportInfoGetter::get_used_info(other_exports_info_data);
      let target_desc = match target {
        Some(resolve_target) => {
          format!(" -> {}", request_shortener(&resolve_target.module))
        }
        None => "".into(),
      };

      let other_export_str =
        format!(r#"{ident}{title} [{provide_info}] [{used_info}]{target_desc}"#,);

      source.add(RawStringSource::from(to_comment_with_nl(&other_export_str)))
    }
  }
}

impl ModuleInfoHeaderPlugin {
  pub fn new(verbose: bool) -> ModuleInfoHeaderPlugin {
    Self::new_inner(verbose)
  }

  pub fn generate_header(module: &dyn Module, compilation: &Compilation) -> String {
    let req = module.readable_identifier(&compilation.options.context);
    let req = COMMENT_END_REGEX.replace_all(&req, "*_/");

    let req_stars_str = "*".repeat(req.len());

    format!("\n/*!****{req_stars_str}****!*\\\n  !*** {req} ***!\n  \\****{req_stars_str}****/\n")
  }
}

#[plugin_hook(CompilerCompilation for ModuleInfoHeaderPlugin)]
async fn compilation(
  &self,
  compilation: &mut Compilation,
  _params: &mut CompilationParams,
) -> Result<()> {
  let mut js_hooks = JsPlugin::get_compilation_hooks_mut(compilation.id());
  js_hooks
    .render_module_package
    .tap(render_js_module_package::new(self));
  js_hooks.chunk_hash.tap(chunk_hash::new(self));

  let mut css_hooks = CssPlugin::get_compilation_hooks_mut(compilation.id());
  css_hooks
    .render_module_package
    .tap(render_css_module_package::new(self));

  Ok(())
}

#[plugin_hook(JavascriptModulesChunkHash for ModuleInfoHeaderPlugin)]
async fn chunk_hash(
  &self,
  _compilation: &Compilation,
  _chunk_ukey: &ChunkUkey,
  hasher: &mut RspackHash,
) -> Result<()> {
  "ModuleInfoHeaderPlugin".hash(hasher);
  "1".hash(hasher);

  Ok(())
}

#[plugin_hook(CssModulesRenderModulePackage for ModuleInfoHeaderPlugin,tracing=false)]
async fn render_css_module_package(
  &self,
  compilation: &Compilation,
  _chunk_key: &ChunkUkey,
  module: &dyn Module,
  render_source: &mut CssModulesRenderSource,
) -> Result<()> {
  let mut new_source: ConcatSource = Default::default();

  new_source.add(RawStringSource::from(
    ModuleInfoHeaderPlugin::generate_header(module, compilation),
  ));

  new_source.add(render_source.source.clone());
  render_source.source = new_source.boxed();

  Ok(())
}

#[plugin_hook(JavascriptModulesRenderModulePackage for ModuleInfoHeaderPlugin,tracing=false)]
async fn render_js_module_package(
  &self,
  compilation: &Compilation,
  chunk_key: &ChunkUkey,
  module: &BoxModule,
  render_source: &mut RenderSource,
  _init_fragments: &mut ChunkInitFragments,
) -> Result<()> {
  let mut new_source: ConcatSource = Default::default();

  new_source.add(RawStringSource::from(
    ModuleInfoHeaderPlugin::generate_header(module.as_ref(), compilation),
  ));

  if self.verbose {
    let export_type = module.build_meta().exports_type;

    new_source.add(RawStringSource::from(to_comment_with_nl(
      &module.build_meta().exports_type.to_string(),
    )));

    let module_graph = compilation.get_module_graph();

    let exports_info = module_graph.get_exports_info(&module.identifier());

    if !matches!(export_type, BuildMetaExportsType::Unset) {
      let request_shortener = |id: &ModuleIdentifier| {
        module_graph
          .module_by_identifier(id)
          .expect("target module should exists")
          .readable_identifier(&compilation.options.context)
          .to_string()
      };

      print_exports_info_to_source(
        &mut new_source,
        "",
        exports_info,
        &request_shortener,
        &mut FxHashSet::default(),
        &module_graph,
      );
    }

    let chunk = compilation
      .chunk_by_ukey
      .get(chunk_key)
      .expect("Chunk must exists");

    if let Some(runtime_requirements) =
      ChunkGraph::get_module_runtime_requirements(compilation, module.identifier(), chunk.runtime())
    {
      let reqs = {
        let mut rr = runtime_requirements
          .iter()
          .map(|v| v.name().to_string())
          .collect::<Vec<_>>();
        rr.sort_by(|a, b| b.cmp(a));
        rr.join(", ")
      };

      new_source.add(RawStringSource::from(to_comment_with_nl(&format!(
        "runtime requirements: {reqs}"
      ))));
    }

    for b in module_graph.get_optimization_bailout(&module.identifier()) {
      new_source.add(RawStringSource::from(to_comment_with_nl(b)))
    }
  }

  new_source.add(render_source.source.clone());

  render_source.source = new_source.boxed();

  Ok(())
}

#[async_trait]
impl Plugin for ModuleInfoHeaderPlugin {
  fn name(&self) -> &'static str {
    "rspack.ModuleInfoHeaderPlugin"
  }

  fn apply(&self, ctx: PluginContext<&mut ApplyContext>, _options: &CompilerOptions) -> Result<()> {
    ctx
      .context
      .compiler_hooks
      .compilation
      .tap(compilation::new(self));
    Ok(())
  }
}
