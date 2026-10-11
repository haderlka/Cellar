//! Help while typing a formula, like Excel's Formula AutoComplete: a list
//! of functions and names matching what is being typed (Tab inserts the
//! highlighted one), and the arguments of the function the caret is in.

use eframe::egui::{self, Context, Key, Modifiers, Rect, RichText, TextEdit};

use crate::app::GuiApp;

/// Functions with their arguments and a one-line description. Functions
/// missing here are still suggested, without arguments.
const FUNCTIONS: &[(&str, &str, &str)] = &[
    ("ABS", "number", "Absolute value"),
    ("ACOS", "number", "Arccosine, in radians"),
    ("AND", "logical1, [logical2], …", "TRUE if every argument is TRUE"),
    ("ARRAYTOTEXT", "array", "The values of an array as text"),
    ("ASIN", "number", "Arcsine, in radians"),
    ("ATAN", "number", "Arctangent, in radians"),
    ("ATAN2", "x, y", "Arctangent of y/x, in radians"),
    ("AVERAGE", "number1, [number2], …", "Arithmetic mean"),
    ("AVERAGEIF", "range, criteria, [average_range]", "Mean of the cells that meet a condition"),
    ("BYCOL", "array, lambda", "Applies a LAMBDA to each column"),
    ("BYROW", "array, lambda", "Applies a LAMBDA to each row"),
    ("CEILING", "number, [significance]", "Rounds up to a multiple"),
    ("CHAR", "number", "The character with that code"),
    ("CLEAN", "text", "Removes non-printable characters"),
    ("CODE", "text", "The code of the first character"),
    ("COMBIN", "n, k", "Number of combinations"),
    ("CONCAT", "text1, [text2], …", "Joins text"),
    ("CORREL", "array1, array2", "Correlation coefficient"),
    ("COS", "number", "Cosine of an angle in radians"),
    ("COSH", "number", "Hyperbolic cosine"),
    ("COUNT", "value1, [value2], …", "Counts the numbers"),
    ("COUNTA", "value1, [value2], …", "Counts the non-empty values"),
    ("COUNTIF", "range, criteria", "Counts the cells that meet a condition"),
    ("DATE", "year, month, day", "A date from its parts"),
    ("DATEDIF", "start_date, end_date, unit", "Difference between dates in \"Y\", \"M\" or \"D\""),
    ("DATEVALUE", "date_text", "A date written as text, as a date"),
    ("DAY", "date", "Day of the month"),
    ("DAYS", "end_date, start_date", "Days between two dates"),
    ("DEGREES", "radians", "Radians to degrees"),
    ("DOLLAR", "number, [decimals]", "A number as currency text"),
    ("EDATE", "start_date, months", "The date a number of months later"),
    ("EOMONTH", "start_date, months", "The last day of a month"),
    ("ERROR.TYPE", "value", "A number for each kind of error"),
    ("EVEN", "number", "Rounds up to an even integer"),
    ("EXACT", "text1, text2", "TRUE if two texts match exactly"),
    ("EXP", "number", "e raised to a power"),
    ("FACT", "number", "Factorial"),
    ("FALSE", "", "The logical value FALSE"),
    ("FILTER", "array, include, [if_empty]", "The rows that meet a condition"),
    ("FIND", "find_text, within_text, [start]", "Position of text (case-sensitive)"),
    ("FIXED", "number, [decimals], [no_commas]", "A number as text with fixed decimals"),
    ("FLOOR", "number, [significance]", "Rounds down to a multiple"),
    ("FREQUENCY", "data, bins", "How many values fall in each bin"),
    ("FV", "rate, nper, pmt, [pv], [type]", "Future value of an investment"),
    ("GCD", "number1, [number2], …", "Greatest common divisor"),
    ("GET", "url", "The text a web address returns"),
    ("GETPIVOTDATA", "data_field, pivot_table, [field, item], …", "A value from a PivotTable"),
    ("HLOOKUP", "lookup_value, table, row_index, [range_lookup]", "Looks up a value in the top row"),
    ("HOUR", "time", "Hour of a time"),
    ("HYPERLINK", "link, [text]", "A link; Ctrl+click opens it"),
    ("IF", "condition, value_if_true, [value_if_false]", "One value or another, by a condition"),
    ("IFERROR", "value, value_if_error", "A fallback when a formula is an error"),
    ("IFNA", "value, value_if_na", "A fallback when a formula is #N/A"),
    ("IFS", "condition1, value1, [condition2, value2], …", "The value of the first true condition"),
    ("INDEX", "array, row, [column]", "The value at a position in a range"),
    ("INDIRECT", "ref_text", "The reference a text names"),
    ("INT", "number", "Rounds down to an integer"),
    ("ISBLANK", "value", "TRUE if the cell is empty"),
    ("ISERR", "value", "TRUE for any error except #N/A"),
    ("ISERROR", "value", "TRUE for any error"),
    ("ISNA", "value", "TRUE for #N/A"),
    ("ISNUMBER", "value", "TRUE if the value is a number"),
    ("ISTEXT", "value", "TRUE if the value is text"),
    ("LAMBDA", "parameter, …, calculation", "A function of your own"),
    ("LARGE", "array, k", "The k-th largest value"),
    ("LCM", "number1, [number2], …", "Least common multiple"),
    ("LEFT", "text, [num_chars]", "The first characters of a text"),
    ("LEN", "text", "Number of characters"),
    ("LET", "name1, value1, …, calculation", "Names values inside a formula"),
    ("LN", "number", "Natural logarithm"),
    ("LOG", "number, [base]", "Logarithm, base 10 unless given"),
    ("LOWER", "text", "Text in lower case"),
    ("MAKEARRAY", "rows, columns, lambda", "An array computed by a LAMBDA"),
    ("MAP", "array, lambda", "Applies a LAMBDA to each value"),
    ("MATCH", "lookup_value, lookup_array, [match_type]", "Position of a value in a range"),
    ("MAX", "number1, [number2], …", "Largest value"),
    ("MEDIAN", "number1, [number2], …", "Middle value"),
    ("MID", "text, start, num_chars", "Characters from the middle of a text"),
    ("MIN", "number1, [number2], …", "Smallest value"),
    ("MINUTE", "time", "Minute of a time"),
    ("MOD", "number, divisor", "Remainder of a division"),
    ("MONTH", "date", "Month of a date"),
    ("MROUND", "number, multiple", "Rounds to a multiple"),
    ("NA", "", "The error #N/A"),
    ("NETWORKDAYS", "start_date, end_date, [holidays]", "Working days between two dates"),
    ("NOT", "logical", "Reverses TRUE and FALSE"),
    ("NOW", "", "The current date and time"),
    ("NPV", "rate, value1, [value2], …", "Net present value"),
    ("NUMBERVALUE", "text, [decimal_separator], [group_separator]", "Text as a number"),
    ("ODD", "number", "Rounds up to an odd integer"),
    ("OFFSET", "reference, rows, cols, [height], [width]", "A range shifted from a reference"),
    ("OR", "logical1, [logical2], …", "TRUE if any argument is TRUE"),
    ("PERCENTILE.INC", "array, k", "The k-th percentile"),
    ("PI", "", "The number pi"),
    ("PMT", "rate, nper, pv, [fv], [type]", "Payment for a loan"),
    ("POWER", "number, power", "A number raised to a power"),
    ("PROPER", "text", "Capitalises each word"),
    ("PV", "rate, nper, pmt, [fv], [type]", "Present value of an investment"),
    ("RADIANS", "degrees", "Degrees to radians"),
    ("RAND", "", "A random number from 0 to 1"),
    ("RANDBETWEEN", "bottom, top", "A random integer in a range"),
    ("RANK.EQ", "number, ref, [order]", "Rank of a number in a list"),
    ("REDUCE", "initial_value, array, lambda", "Folds an array into one value"),
    ("REGEXEXTRACT", "text, pattern", "The first match of a regular expression"),
    ("REGEXMATCH", "text, pattern", "TRUE if a regular expression matches"),
    ("REGEXREPLACE", "text, pattern, replacement", "Replaces regular-expression matches"),
    ("REPLACE", "old_text, start, num_chars, new_text", "Replaces characters by position"),
    ("REPT", "text, times", "Repeats text"),
    ("RIGHT", "text, [num_chars]", "The last characters of a text"),
    ("ROUND", "number, digits", "Rounds to a number of digits"),
    ("ROUNDDOWN", "number, digits", "Rounds towards zero"),
    ("ROUNDUP", "number, digits", "Rounds away from zero"),
    ("SCAN", "initial_value, array, lambda", "Running results of a LAMBDA"),
    ("SEARCH", "find_text, within_text, [start]", "Position of text (ignores case)"),
    ("SECOND", "time", "Second of a time"),
    ("SEQUENCE", "rows, [columns], [start], [step]", "A list of numbers"),
    ("SIGN", "number", "1, 0 or -1 by the sign"),
    ("SIN", "number", "Sine of an angle in radians"),
    ("SINH", "number", "Hyperbolic sine"),
    ("SMALL", "array, k", "The k-th smallest value"),
    ("SORT", "array, [sort_index], [sort_order]", "A range, sorted"),
    ("SPARKLINE", "data, [type]", "A small chart in the cell: \"line\" or \"column\""),
    ("SQRT", "number", "Square root"),
    ("STDEV", "number1, [number2], …", "Standard deviation of a sample"),
    ("STDEV.P", "number1, [number2], …", "Standard deviation of a population"),
    ("STDEV.S", "number1, [number2], …", "Standard deviation of a sample"),
    ("SUBSTITUTE", "text, old_text, new_text, [instance]", "Replaces text with other text"),
    ("SUM", "number1, [number2], …", "Adds numbers"),
    ("SUMIF", "range, criteria, [sum_range]", "Adds the cells that meet a condition"),
    ("SUMPRODUCT", "array1, [array2], …", "Sum of the products of ranges"),
    ("SWITCH", "expression, value1, result1, …, [default]", "The result for the matching value"),
    ("TAN", "number", "Tangent of an angle in radians"),
    ("TANH", "number", "Hyperbolic tangent"),
    ("TEXT", "value, format_text", "A value as text in a number format"),
    ("TEXTAFTER", "text, delimiter", "The text after a delimiter"),
    ("TEXTBEFORE", "text, delimiter", "The text before a delimiter"),
    ("TEXTJOIN", "delimiter, ignore_empty, text1, …", "Joins text with a delimiter"),
    ("TIME", "hour, minute, second", "A time from its parts"),
    ("TIMEVALUE", "time_text", "A time written as text, as a time"),
    ("TODAY", "", "Today's date"),
    ("TRANSPOSE", "array", "Swaps rows and columns"),
    ("TRIM", "text", "Removes extra spaces"),
    ("TRUE", "", "The logical value TRUE"),
    ("TRUNC", "number, [digits]", "Cuts off decimals"),
    ("TYPE", "value", "A number for the kind of value"),
    ("UNICHAR", "number", "The Unicode character with that number"),
    ("UNICODE", "text", "The Unicode number of the first character"),
    ("UNIQUE", "array", "The distinct values of a range"),
    ("UPPER", "text", "Text in upper case"),
    ("VALUE", "text", "Text as a number"),
    ("VAR.P", "number1, [number2], …", "Variance of a population"),
    ("VAR.S", "number1, [number2], …", "Variance of a sample"),
    ("VLOOKUP", "lookup_value, table, col_index, [range_lookup]", "Looks up a value in the first column"),
    ("WEEKDAY", "date, [type]", "Day of the week as a number"),
    ("WORKDAY", "start_date, days, [holidays]", "The date a number of working days later"),
    ("XLOOKUP", "lookup_value, lookup_array, return_array, [if_not_found]", "Looks up a value in one range, returns from another"),
    ("XOR", "logical1, [logical2], …", "TRUE if an odd number of arguments are TRUE"),
    ("YEAR", "date", "Year of a date"),
    ("YEARFRAC", "start_date, end_date, [basis]", "Fraction of a year between two dates"),
];

fn function_info(name: &str) -> Option<&'static (&'static str, &'static str, &'static str)> {
    FUNCTIONS.iter().find(|f| f.0.eq_ignore_ascii_case(name))
}

/// What the suggestion list offers this frame; `handle_assist_keys` acts
/// on it the next.
pub struct Assist {
    items: Vec<String>,
    selected: usize,
    /// Byte range of the text a chosen item replaces.
    replace: std::ops::Range<usize>,
}

/// What the text before the caret calls for.
#[derive(Debug, PartialEq)]
enum Typing {
    /// A name being typed, starting at this byte.
    Name(usize),
    /// Inside the brackets of this function, at this argument (0-based).
    Call(String, usize),
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.'
}

fn typing_context(before: &str) -> Option<Typing> {
    // Open calls as (function name, argument index); no help inside quotes.
    let mut calls: Vec<(String, usize)> = Vec::new();
    let mut in_string = false;
    let mut name_start = 0;
    for (i, ch) in before.char_indices() {
        if ch == '"' {
            in_string = !in_string;
        }
        if in_string {
            continue;
        }
        match ch {
            '(' => calls.push((before[name_start..i].to_string(), 0)),
            ')' => {
                calls.pop();
            }
            ',' | ';' => {
                if let Some(call) = calls.last_mut() {
                    call.1 += 1;
                }
            }
            _ => {}
        }
        if !is_name_char(ch) {
            name_start = i + ch.len_utf8();
        }
    }
    if in_string {
        return None;
    }
    let name = &before[name_start..];
    // `A1:B` and `Sheet2!B` are references being typed, not names.
    let after_ref = before[..name_start].ends_with([':', '!', '$']);
    if name.chars().next().is_some_and(char::is_alphabetic) && !after_ref {
        return Some(Typing::Name(name_start));
    }
    calls.pop().filter(|(name, _)| !name.is_empty()).map(|(name, arg)| Typing::Call(name, arg))
}

impl GuiApp {
    /// Names offered for `prefix`: functions first, then the workbook's
    /// own names and tables.
    fn suggestions(&self, prefix: &str) -> Vec<String> {
        let prefix = prefix.to_uppercase();
        let mut functions: Vec<String> = cellar::domain::builtin_function_names()
            .into_iter()
            .filter(|n| n.starts_with(&prefix))
            .map(str::to_string)
            .collect();
        functions.sort();
        functions.dedup();
        let wb = &self.app.workbook;
        let mut names: Vec<String> = self
            .app
            .user_names()
            .into_iter()
            .map(|(name, _)| name)
            .chain(wb.sheets.iter().flat_map(|s| s.tables.iter().map(|t| t.name.clone())))
            .filter(|n| n.to_uppercase().starts_with(&prefix))
            .collect();
        names.sort();
        functions.extend(names);
        functions.truncate(8);
        functions
    }

    /// Show formula help under the editor `id` at `anchor`.
    pub fn formula_assist(&mut self, ctx: &Context, id: egui::Id, anchor: Rect) {
        let previous = self.assist_previous.take();
        let Some(edit) = self.edit.as_ref().filter(|e| e.text.starts_with('=')) else { return };
        let chars = edit.text.chars().count();
        let caret = TextEdit::load_state(ctx, id)
            .and_then(|state| state.cursor.char_range())
            .map_or(chars, |range| usize::from(range.primary.index).min(chars));
        let caret_byte = edit.text.char_indices().nth(caret).map_or(edit.text.len(), |(i, _)| i);
        let before = edit.text[..caret_byte].to_string();
        let pos = anchor.left_bottom() + egui::vec2(0.0, 2.0);

        match typing_context(&before) {
            Some(Typing::Name(start)) => {
                let items = self.suggestions(&before[start..]);
                if items.is_empty() {
                    return;
                }
                // Keep the highlighted row while the list is the same.
                let selected = previous.filter(|p| p.items == items).map_or(0, |p| p.selected);
                let mut clicked = None;
                let area = egui::Area::new(egui::Id::new("formula_assist")).order(egui::Order::Foreground).fixed_pos(pos).show(
                    ctx,
                    |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            for (i, item) in items.iter().enumerate() {
                                if ui.selectable_label(i == selected, RichText::new(item).monospace()).clicked() {
                                    clicked = Some(i);
                                }
                            }
                            if let Some((_, args, about)) = function_info(&items[selected]) {
                                ui.separator();
                                ui.label(RichText::new(format!("{}({})", items[selected], args)).small());
                                ui.label(RichText::new(*about).small().weak());
                            }
                            ui.label(RichText::new("Tab inserts").small().weak());
                        });
                    },
                );
                self.grid.overlays.push(area.response.rect);
                self.assist = Some(Assist { items, selected, replace: start..caret_byte });
                if let Some(i) = clicked {
                    self.accept_suggestion(i);
                }
            }
            Some(Typing::Call(name, arg)) => {
                let Some((name, args, about)) = function_info(&name) else { return };
                if args.is_empty() {
                    return;
                }
                let parts: Vec<&str> = args.split(", ").collect();
                let current = arg.min(parts.len() - 1);
                egui::Area::new(egui::Id::new("formula_assist"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(pos)
                    .interactable(false)
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                ui.label(RichText::new(format!("{}(", name)).monospace());
                                for (i, part) in parts.iter().enumerate() {
                                    let text = RichText::new(*part).monospace();
                                    ui.label(if i == current { text.strong() } else { text.weak() });
                                    ui.label(RichText::new(if i + 1 < parts.len() { ", " } else { ")" }).monospace());
                                }
                            });
                            ui.label(RichText::new(*about).small().weak());
                        });
                    });
            }
            None => {}
        }
    }

    /// Put the suggestion into the formula in place of what was typed.
    fn accept_suggestion(&mut self, index: usize) {
        let Some(assist) = self.assist.take() else { return };
        let (Some(item), Some(edit)) = (assist.items.get(index), &mut self.edit) else { return };
        if assist.replace.end > edit.text.len() || !edit.text.is_char_boundary(assist.replace.end) {
            return;
        }
        let is_function = cellar::domain::builtin_function_names().contains(&item.as_str());
        // Don't double a bracket that is already there.
        let has_bracket = edit.text[assist.replace.end..].starts_with('(');
        let insert = if is_function && !has_bracket { format!("{}(", item) } else { item.clone() };
        edit.text.replace_range(assist.replace.clone(), &insert);
        edit.caret = Some(edit.text[..assist.replace.start + insert.len()].chars().count());
        edit.ref_start = None;
        edit.focus_pending = true;
    }

    /// Keys for the suggestion list: arrows move the highlight, Tab
    /// inserts it. Returns whether Tab was used.
    pub fn handle_assist_keys(&mut self, ctx: &Context) -> bool {
        let Some(assist) = &mut self.assist else { return false };
        let n = assist.items.len();
        let (tab, down, up) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Tab),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
            )
        });
        if down {
            assist.selected = (assist.selected + 1) % n;
        }
        if up {
            assist.selected = (assist.selected + n - 1) % n;
        }
        if tab {
            let selected = assist.selected;
            self.accept_suggestion(selected);
        }
        tab
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_is_a_name_or_the_enclosing_call() {
        assert_eq!(typing_context("=SU"), Some(Typing::Name(1)));
        assert_eq!(typing_context("=SUM(A1:A5, vl"), Some(Typing::Name(12)));
        assert_eq!(typing_context("=SUM("), Some(Typing::Call("SUM".into(), 0)));
        assert_eq!(typing_context("=IF(A1>2, MAX(1, 2), "), Some(Typing::Call("IF".into(), 2)));
        assert_eq!(typing_context("=IF(A1>2, MAX(1, "), Some(Typing::Call("MAX".into(), 1)));
        assert_eq!(typing_context("=IF(A1=\"a, b(\", "), Some(Typing::Call("IF".into(), 1)));
        assert_eq!(typing_context("=\"SU"), None, "inside quotes");
        assert_eq!(typing_context("=A1:B"), None, "the end of a range");
        assert_eq!(typing_context("=(1+"), None, "plain brackets");
        assert_eq!(typing_context("=1+2"), None);
    }

    #[test]
    fn every_documented_function_exists() {
        let names = cellar::domain::builtin_function_names();
        for (name, _, _) in FUNCTIONS {
            assert!(names.contains(name), "{} is documented but not a function", name);
        }
        assert!(FUNCTIONS.windows(2).all(|w| w[0].0 < w[1].0), "keep the table sorted");
    }
}
