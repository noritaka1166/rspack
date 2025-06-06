use rspack_core::{DependencyRange, SpanExt};
use swc_core::{
  common::Spanned,
  ecma::ast::{BinExpr, BinaryOp},
};

use crate::{utils::eval::BasicEvaluatedExpression, visitors::JavascriptParser};

#[inline]
fn handle_template_string_compare<'a>(
  left: &BasicEvaluatedExpression,
  right: &BasicEvaluatedExpression,
  mut res: BasicEvaluatedExpression<'a>,
  eql: bool,
) -> Option<BasicEvaluatedExpression<'a>> {
  let get_prefix = |parts: &Vec<BasicEvaluatedExpression>| {
    let mut value = vec![];
    for p in parts {
      if let Some(s) = p.as_string() {
        value.push(s);
      } else {
        break;
      }
    }
    value.concat()
  };
  let get_suffix = |parts: &Vec<BasicEvaluatedExpression>| {
    let mut value = vec![];
    for p in parts.iter().rev() {
      if let Some(s) = p.as_string() {
        value.push(s);
      } else {
        break;
      }
    }
    value.concat()
  };

  let prefix_res = {
    let left_prefix = get_prefix(left.parts());
    let right_prefix = get_prefix(right.parts());
    let len_prefix = usize::min(left_prefix.len(), right_prefix.len());
    len_prefix > 0 && left_prefix[0..len_prefix] != right_prefix[0..len_prefix]
  };
  if prefix_res {
    res.set_bool(!eql);
    res.set_side_effects(left.could_have_side_effects() || right.could_have_side_effects());
    return Some(res);
  }

  let suffix_res = {
    let left_suffix = get_suffix(left.parts());
    let right_suffix = get_suffix(right.parts());
    let len_suffix = usize::min(left_suffix.len(), right_suffix.len());
    len_suffix > 0
      && left_suffix[left_suffix.len() - len_suffix..]
        != right_suffix[right_suffix.len() - len_suffix..]
  };
  if suffix_res {
    res.set_bool(!eql);
    res.set_side_effects(left.could_have_side_effects() || right.could_have_side_effects());
    return Some(res);
  }

  None
}

#[inline]
fn is_always_different(a: Option<bool>, b: Option<bool>) -> bool {
  match (a, b) {
    (Some(a), Some(b)) => a != b,
    _ => false,
  }
}

/// `eql` is `true` for `===` and `false` for `!==`
#[inline]
fn handle_strict_equality_comparison<'a>(
  eql: bool,
  left: BasicEvaluatedExpression<'a>,
  expr: &'a BinExpr,
  scanner: &mut JavascriptParser,
) -> Option<BasicEvaluatedExpression<'a>> {
  assert!(expr.op == BinaryOp::EqEqEq || expr.op == BinaryOp::NotEqEq);
  let right = scanner.evaluate_expression(&expr.right);
  let mut res = BasicEvaluatedExpression::with_range(expr.span.real_lo(), expr.span.hi().0);
  let left_const = left.is_compile_time_value();
  let right_const = right.is_compile_time_value();

  let common = |mut res: BasicEvaluatedExpression<'a>| {
    res.set_bool(!eql);
    res.set_side_effects(left.could_have_side_effects() || right.could_have_side_effects());
    Some(res)
  };

  if left_const && right_const {
    res.set_bool(eql == left.compare_compile_time_value(&right));
    res.set_side_effects(left.could_have_side_effects() || right.could_have_side_effects());
    Some(res)
  } else if left.is_array() && right.is_array() {
    common(res)
  } else if left.is_template_string() && right.is_template_string() {
    handle_template_string_compare(&left, &right, res, eql)
  } else if is_always_different(left.as_bool(), right.as_bool())
    || is_always_different(left.as_nullish(), right.as_nullish())
  {
    common(res)
  } else {
    let left_primitive = left.is_primitive_type();
    let right_primitive = right.is_primitive_type();
    if left_primitive == Some(false) && (left_const || right_primitive == Some(true))
      || (right_primitive == Some(false) && (right_const || left_primitive == Some(true)))
    {
      common(res)
    } else {
      None
    }
  }
}

/// `eql` is `true` for `==` and `false` for `!=`
#[inline(always)]
fn handle_abstract_equality_comparison<'a>(
  eql: bool,
  left: BasicEvaluatedExpression<'a>,
  expr: &'a BinExpr,
  scanner: &mut JavascriptParser,
) -> Option<BasicEvaluatedExpression<'a>> {
  assert!(expr.op == BinaryOp::EqEq || expr.op == BinaryOp::NotEq);
  let right = scanner.evaluate_expression(&expr.right);
  let mut res = BasicEvaluatedExpression::with_range(expr.span.real_lo(), expr.span.hi().0);

  let left_const = left.is_compile_time_value();
  let right_const = right.is_compile_time_value();

  if left_const && right_const {
    res.set_bool(eql == left.compare_compile_time_value(&right));
    res.set_side_effects(left.could_have_side_effects() || right.could_have_side_effects());
    Some(res)
  } else if left.is_array() && right.is_array() {
    res.set_bool(!eql);
    res.set_side_effects(left.could_have_side_effects() || right.could_have_side_effects());
    Some(res)
  } else if left.is_template_string() && right.is_template_string() {
    handle_template_string_compare(&left, &right, res, eql)
  } else {
    None
  }
}

#[inline(always)]
fn handle_nullish_coalescing<'a>(
  left: BasicEvaluatedExpression<'a>,
  expr: &'a BinExpr,
  scanner: &mut JavascriptParser,
) -> Option<BasicEvaluatedExpression<'a>> {
  let left_nullish = left.as_nullish();
  match left_nullish {
    Some(true) => {
      let mut right = scanner.evaluate_expression(&expr.right);
      if left.could_have_side_effects() {
        right.set_side_effects(true)
      }
      right.set_range(expr.span.real_lo(), expr.span.hi().0);
      Some(right)
    }
    Some(false) => {
      let mut res = left.clone();
      res.set_range(expr.span.real_lo(), expr.span.hi().0);
      Some(res)
    }
    _ => None,
  }
}

#[inline(always)]
fn handle_logical_or<'a>(
  left: BasicEvaluatedExpression<'a>,
  expr: &'a BinExpr,
  scanner: &mut JavascriptParser,
) -> Option<BasicEvaluatedExpression<'a>> {
  let left_bool = left.as_bool();
  match left_bool {
    Some(true) => {
      let mut res = left.clone();
      res.set_range(expr.span.real_lo(), expr.span.hi().0);
      Some(res)
    }
    Some(false) => {
      let mut right = scanner.evaluate_expression(&expr.right);
      if left.could_have_side_effects() {
        right.set_side_effects(true)
      }
      right.set_range(expr.span.real_lo(), expr.span.hi().0);
      Some(right)
    }
    _ => {
      let right_bool = scanner.evaluate_expression(&expr.right).as_bool();
      if right_bool.is_some_and(|x| x) {
        let mut res = BasicEvaluatedExpression::with_range(expr.span.real_lo(), expr.span.hi().0);
        res.set_truthy();
        Some(res)
      } else {
        None
      }
    }
  }
}

#[inline(always)]
fn handle_logical_and<'a>(
  left: BasicEvaluatedExpression<'a>,
  expr: &'a BinExpr,
  scanner: &mut JavascriptParser,
) -> Option<BasicEvaluatedExpression<'a>> {
  let left_bool = left.as_bool();
  match left_bool {
    Some(true) => {
      let mut right = scanner.evaluate_expression(&expr.right);
      if left.could_have_side_effects() {
        right.set_side_effects(true)
      }
      right.set_range(expr.span.real_lo(), expr.span.hi().0);
      Some(right)
    }
    Some(false) => {
      let mut res = left.clone();
      res.set_range(expr.span.real_lo(), expr.span.hi().0);
      Some(res)
    }
    None => {
      let right_bool = scanner.evaluate_expression(&expr.right).as_bool();
      if right_bool.is_some_and(|x| !x) {
        let mut res = BasicEvaluatedExpression::with_range(expr.span.real_lo(), expr.span.hi().0);
        res.set_falsy();
        Some(res)
      } else {
        None
      }
    }
  }
}

#[inline(always)]
fn handle_add<'a>(
  left: BasicEvaluatedExpression<'a>,
  expr: &'a BinExpr,
  scanner: &mut JavascriptParser,
) -> Option<BasicEvaluatedExpression<'a>> {
  assert_eq!(expr.op, BinaryOp::Add);
  let right = scanner.evaluate_expression(&expr.right);
  let mut res = BasicEvaluatedExpression::with_range(expr.span.real_lo(), expr.span.hi.0);
  if left.could_have_side_effects() || right.could_have_side_effects() {
    res.set_side_effects(true)
  }
  if left.is_string() {
    if right.is_string() {
      res.set_string(format!("{}{}", left.string(), right.string()));
    } else if right.is_number() {
      res.set_string(format!("{}{}", left.string(), right.number()));
    } else if right.is_wrapped()
      && let Some(prefix) = right.prefix()
      && prefix.is_string()
    {
      let (start, end) = join_locations(left.range.as_ref(), prefix.range.as_ref());
      let mut left_prefix = BasicEvaluatedExpression::with_range(start, end);
      left_prefix.set_string(format!("{}{}", left.string(), prefix.string()));
      res.set_wrapped(
        Some(left_prefix),
        right.postfix.map(|postfix| *postfix),
        right
          .wrapped_inner_expressions
          .expect("wrapped_inner_expressions must be exists under wrapped"),
      )
    } else if right.is_wrapped() {
      res.set_wrapped(
        Some(left),
        right.postfix.map(|postfix| *postfix),
        right
          .wrapped_inner_expressions
          .expect("wrapped_inner_expressions must be exists under wrapped"),
      );
    } else {
      res.set_wrapped(Some(left), None, vec![right])
    }
  } else if left.is_number() {
    if right.is_string() {
      res.set_string(format!("{}{}", left.number(), right.string()));
    } else if right.is_number() {
      res.set_number(left.number() + right.number())
    } else {
      return None;
    }
  } else if left.is_bigint() {
    // TODO: handle `left.is_bigint`
    return None;
  } else if left.is_wrapped() {
    if let Some(postfix) = &left.postfix
      && postfix.is_string()
      && right.is_string()
    {
      let range = join_locations(postfix.range.as_ref(), right.range.as_ref());
      let mut right_postfix = BasicEvaluatedExpression::with_range(range.0, range.1);
      right_postfix.set_string(format!("{}{}", postfix.string(), right.string()));
      res.set_wrapped(
        left.prefix.map(|prefix| *prefix),
        Some(right_postfix),
        left
          .wrapped_inner_expressions
          .expect("wrapped_inner_expressions must be exists under wrapped"),
      )
    } else if let Some(postfix) = &left.postfix
      && postfix.is_string()
      && right.is_number()
    {
      let range = join_locations(postfix.range.as_ref(), right.range.as_ref());
      let mut right_postfix = BasicEvaluatedExpression::with_range(range.0, range.1);
      right_postfix.set_string(format!("{}{}", postfix.string(), right.number()));
      res.set_wrapped(
        left.prefix.map(|prefix| *prefix),
        Some(right_postfix),
        left
          .wrapped_inner_expressions
          .expect("wrapped_inner_expressions must be exists under wrapped"),
      )
    } else if right.is_string() {
      res.set_wrapped(
        left.prefix.map(|prefix| *prefix),
        Some(right),
        left
          .wrapped_inner_expressions
          .expect("wrapped_inner_expressions must be exists under wrapped"),
      );
    } else if right.is_number() {
      let range = right.range();
      let mut postfix = BasicEvaluatedExpression::with_range(range.0, range.1);
      postfix.set_string(right.number().to_string());
      res.set_wrapped(
        left.prefix.map(|prefix| *prefix),
        Some(postfix),
        left
          .wrapped_inner_expressions
          .expect("wrapped_inner_expressions must be exists under wrapped"),
      )
    } else if right.is_wrapped() {
      let inner_expressions = if let Some(mut left_inner_expression) =
        left.wrapped_inner_expressions
        && let Some(mut right_inner_expression) = right.wrapped_inner_expressions
      {
        if let Some(postfix) = left.postfix {
          left_inner_expression.push(*postfix);
        }
        if let Some(prefix) = right.prefix {
          left_inner_expression.push(*prefix);
        }
        left_inner_expression.append(&mut right_inner_expression);
        left_inner_expression
      } else {
        vec![]
      };
      res.set_wrapped(
        left.prefix.map(|prefix| *prefix),
        right.postfix.map(|postfix| *postfix),
        inner_expressions,
      );
    } else {
      let inner_expressions =
        if let Some(mut left_inner_expression) = left.wrapped_inner_expressions {
          if let Some(postfix) = left.postfix {
            left_inner_expression.push(*postfix);
          }
          left_inner_expression.push(right);
          left_inner_expression
        } else {
          vec![]
        };
      res.set_wrapped(left.prefix.map(|prefix| *prefix), None, inner_expressions)
    }
  } else if right.is_string() {
    res.set_wrapped(None, Some(right), vec![left]);
  } else if right.is_wrapped() {
    let mut inner_expressions = if let Some(right_prefix) = right.prefix {
      vec![left, *right_prefix]
    } else {
      vec![left]
    };
    if let Some(mut right_inner_expressions) = right.wrapped_inner_expressions {
      inner_expressions.append(&mut right_inner_expressions)
    }
    res.set_wrapped(
      None,
      right.postfix.map(|postfix| *postfix),
      inner_expressions,
    );
  } else {
    return None;
  }

  Some(res)
}

#[inline(always)]
pub fn handle_const_operation<'a>(
  left: BasicEvaluatedExpression<'a>,
  expr: &'a BinExpr,
  scanner: &mut JavascriptParser,
) -> Option<BasicEvaluatedExpression<'a>> {
  if !left.is_compile_time_value() {
    return None;
  }
  let right = scanner.evaluate_expression(&expr.right);
  if !right.is_compile_time_value() {
    return None;
  }

  let mut res = BasicEvaluatedExpression::with_range(expr.span.real_lo(), expr.span.hi().0);
  res.set_side_effects(left.could_have_side_effects() || right.could_have_side_effects());

  match expr.op {
    BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Exp => {
      if let Some(left_number) = left.as_number()
        && let Some(right_number) = right.as_number()
      {
        res.set_number(match expr.op {
          BinaryOp::Sub => left_number - right_number,
          BinaryOp::Mul => left_number * right_number,
          BinaryOp::Div => left_number / right_number,
          BinaryOp::Exp => left_number.powf(right_number),
          _ => unreachable!(),
        });
        Some(res)
      } else {
        None
      }
    }
    BinaryOp::LShift | BinaryOp::RShift => {
      if let Some(left_number) = left.as_int()
        && let Some(right_number) = right.as_int()
      {
        // only the lower 5 bits are used when shifting, so don't do anything
        // if the shift amount is outside [0,32)
        if (0..32).contains(&right_number) {
          res.set_number(match expr.op {
            BinaryOp::LShift => left_number << right_number,
            BinaryOp::RShift => left_number >> right_number,
            _ => unreachable!(),
          } as f64);
        } else {
          res.set_number(left_number as f64);
        }
        Some(res)
      } else {
        None
      }
    }
    BinaryOp::BitAnd | BinaryOp::BitXor | BinaryOp::BitOr => {
      if let Some(left_number) = left.as_int()
        && let Some(right_number) = right.as_int()
      {
        res.set_number(match expr.op {
          BinaryOp::BitAnd => left_number & right_number,
          BinaryOp::BitXor => left_number ^ right_number,
          BinaryOp::BitOr => left_number | right_number,
          _ => unreachable!(),
        } as f64);
        Some(res)
      } else {
        None
      }
    }
    BinaryOp::Lt | BinaryOp::Gt | BinaryOp::LtEq | BinaryOp::GtEq => {
      if left.is_string() && right.is_string() {
        let left_str = left.string();
        let right_str = right.string();
        res.set_bool(match expr.op {
          BinaryOp::Lt => left_str < right_str,
          BinaryOp::LtEq => left_str <= right_str,
          BinaryOp::Gt => left_str > right_str,
          BinaryOp::GtEq => left_str >= right_str,
          _ => unreachable!(),
        });
        Some(res)
      } else if let Some(left_number) = left.as_number()
        && let Some(right_number) = right.as_number()
      {
        res.set_bool(match expr.op {
          BinaryOp::Lt => left_number < right_number,
          BinaryOp::LtEq => left_number <= right_number,
          BinaryOp::Gt => left_number > right_number,
          BinaryOp::GtEq => left_number >= right_number,
          _ => unreachable!(),
        });
        Some(res)
      } else {
        None
      }
    }
    _ => None,
  }
}

pub fn eval_binary_expression<'a>(
  scanner: &mut JavascriptParser,
  expr: &'a BinExpr,
) -> Option<BasicEvaluatedExpression<'a>> {
  let mut stack = vec![expr];
  let mut expr = &*expr.left;
  while let Some(bin) = expr.as_bin() {
    stack.push(bin);
    expr = &*bin.left;
  }
  let mut evaluated = None;
  while let Some(expr) = stack.pop() {
    let left = evaluated.unwrap_or_else(|| scanner.evaluate_expression(&expr.left));
    evaluated = match expr.op {
      BinaryOp::EqEq => handle_abstract_equality_comparison(true, left, expr, scanner),
      BinaryOp::NotEq => handle_abstract_equality_comparison(false, left, expr, scanner),
      BinaryOp::EqEqEq => handle_strict_equality_comparison(true, left, expr, scanner),
      BinaryOp::NotEqEq => handle_strict_equality_comparison(false, left, expr, scanner),
      BinaryOp::LogicalAnd => handle_logical_and(left, expr, scanner),
      BinaryOp::LogicalOr => handle_logical_or(left, expr, scanner),
      BinaryOp::NullishCoalescing => handle_nullish_coalescing(left, expr, scanner),
      BinaryOp::Add => handle_add(left, expr, scanner),
      _ => handle_const_operation(left, expr, scanner),
    }
    .or_else(|| {
      Some(BasicEvaluatedExpression::with_range(
        expr.span().real_lo(),
        expr.span_hi().0,
      ))
    });
  }
  evaluated
}

fn join_locations(start: Option<&DependencyRange>, end: Option<&DependencyRange>) -> (u32, u32) {
  match (start, end) {
    (None, None) => unreachable!("invalid range"),
    (None, Some(end)) => (end.start, end.end),
    (Some(start), None) => (start.start, start.end),
    (Some(start), Some(end)) => {
      join_ranges(Some((start.start, start.end)), Some((end.start, end.end)))
    }
  }
}

fn join_ranges(start: Option<(u32, u32)>, end: Option<(u32, u32)>) -> (u32, u32) {
  match (start, end) {
    (None, None) => unreachable!("invalid range"),
    (None, Some(end)) => end,
    (Some(start), None) => start,
    (Some(start), Some(end)) => {
      assert!(start.0 <= end.1);
      (start.0, end.1)
    }
  }
}
