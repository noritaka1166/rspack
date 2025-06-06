const checkMap = __non_webpack_require__("../../../../../dist/helper/util/checkSourceMap.js").default;
const fs = __non_webpack_require__("fs");
const path = __non_webpack_require__("path");

try {
	require("./a.js");
} catch (e) {
	// ignore
}

it("verify importing css js source map", async () => {
	const source = fs.readFileSync(__filename + ".map", "utf-8");
	const map = JSON.parse(source);
	expect(map.sources.sort()).toEqual([
		"webpack:///./a.js",
		"webpack:///./index.js",
		"webpack:///webpack/runtime/auto_public_path",
		"webpack:///webpack/runtime/get css chunk filename",
		"webpack:///webpack/runtime/global",
		"webpack:///webpack/runtime/has_own_property",
		"webpack:///webpack/runtime/make_namespace_object",
	]);
	expect(map.file).toEqual("bundle0.js");
	const out = fs.readFileSync(__filename, "utf-8");
	expect(
		await checkMap(out, source, {
			// *${id}* as the search key to avoid conflict with `Object.defineProperty(exports, ${id}, ...)`
			['"*a0*"']: "webpack:///a.js",
			['"*a1*"']: "webpack:///a.js"
		}, false)
	).toBe(true);
});

it("verify css source map", async () => {
	const cssSource = fs.readFileSync(
		path.resolve(__dirname, "bundle0.css.map"),
		"utf-8"
	);
	const cssMap = JSON.parse(cssSource);
	expect(cssMap.sources).toEqual(["webpack:///./a.css"]);
	expect(cssMap.file).toEqual("bundle0.css");
	const cssOut = fs.readFileSync(
		path.resolve(__dirname, "bundle0.css"),
		"utf-8"
	);
	expect(
		await checkMap(cssOut, cssSource, {
			[`a:nth-child(0):after { content: "a0"; }`]: "webpack:///a.css",
			[`a:nth-child(1):after { content: "a1"; }`]: "webpack:///a.css",
			[`a:nth-child(2):after { content: "a2"; }`]: "webpack:///a.css"
		})
	).toBe(true);
});
