//! Submodule of `models` — see models/mod.rs.

#![allow(unused_imports)]
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use super::*;

/// Most decimals a number format shows (Excel's limit). Formats come from
/// hand-editable files; `decimals: 4000000000` would otherwise allocate
/// gigabytes per cell drawn.
pub const MAX_DECIMALS: u32 = 30;

/// Number format for cell display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NumberFormat {
    /// Default rendering (no formatting)
    General,
    /// Fixed decimal places with optional thousands separator
    Number { decimals: u32, thousands_sep: bool },
    /// Currency with symbol and decimal places
    Currency { symbol: String, decimals: u32 },
    /// Percentage (multiply by 100 and add %)
    Percentage { decimals: u32 },
}

impl NumberFormat {
    /// Decimal places shown, or `None` for General.
    pub fn decimals(&self) -> Option<u32> {
        match self {
            NumberFormat::General => None,
            NumberFormat::Number { decimals, .. }
            | NumberFormat::Currency { decimals, .. }
            | NumberFormat::Percentage { decimals } => Some(*decimals),
        }
    }

    /// The same format showing `n` decimal places. General becomes a plain
    /// number format, as Excel's Increase Decimal does.
    pub fn with_decimals(&self, n: u32) -> NumberFormat {
        let decimals = n.min(MAX_DECIMALS);
        match self {
            NumberFormat::General => NumberFormat::Number { decimals, thousands_sep: false },
            NumberFormat::Number { thousands_sep, .. } => NumberFormat::Number { decimals, thousands_sep: *thousands_sep },
            NumberFormat::Currency { symbol, .. } => NumberFormat::Currency { symbol: symbol.clone(), decimals },
            NumberFormat::Percentage { .. } => NumberFormat::Percentage { decimals },
        }
    }
}

/// Cell text/fill colour. A fixed 15-colour palette (inherited from
/// tshts, where it mapped onto terminal colours); imported colours snap to
/// the nearest entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TerminalColor {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    DarkGray,
    LightRed,
    LightGreen,
    LightYellow,
    LightBlue,
    LightMagenta,
    LightCyan,
}

impl TerminalColor {
    /// Every palette entry, in declaration order.
    pub const ALL: [TerminalColor; 15] = [
        Self::Black,
        Self::Red,
        Self::Green,
        Self::Yellow,
        Self::Blue,
        Self::Magenta,
        Self::Cyan,
        Self::White,
        Self::DarkGray,
        Self::LightRed,
        Self::LightGreen,
        Self::LightYellow,
        Self::LightBlue,
        Self::LightMagenta,
        Self::LightCyan,
    ];

    /// RGB used when rendering and when mapping
    /// arbitrary colors (e.g. from .xlsx) onto the palette. The light
    /// variants are pale tints so they work as cell backgrounds.
    pub fn rgb(&self) -> (u8, u8, u8) {
        match self {
            Self::Black => (0, 0, 0),
            Self::Red => (200, 40, 40),
            Self::Green => (40, 150, 60),
            Self::Yellow => (230, 190, 40),
            Self::Blue => (40, 90, 200),
            Self::Magenta => (170, 60, 170),
            Self::Cyan => (40, 160, 180),
            Self::White => (255, 255, 255),
            Self::DarkGray => (110, 110, 110),
            Self::LightRed => (250, 200, 200),
            Self::LightGreen => (200, 235, 200),
            Self::LightYellow => (255, 245, 170),
            Self::LightBlue => (195, 220, 250),
            Self::LightMagenta => (235, 200, 235),
            Self::LightCyan => (195, 235, 240),
        }
    }

    /// Palette entry closest to an arbitrary RGB color.
    pub fn nearest(r: u8, g: u8, b: u8) -> Self {
        let dist = |c: &TerminalColor| {
            let (cr, cg, cb) = c.rgb();
            let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2);
            d(r, cr) + d(g, cg) + d(b, cb)
        };
        Self::ALL.iter().min_by_key(|c| dist(c)).cloned().unwrap_or(Self::Black)
    }

    /// Parses a color name string into a TerminalColor.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "black" => Some(Self::Black),
            "red" => Some(Self::Red),
            "green" => Some(Self::Green),
            "yellow" => Some(Self::Yellow),
            "blue" => Some(Self::Blue),
            "magenta" => Some(Self::Magenta),
            "cyan" => Some(Self::Cyan),
            "white" => Some(Self::White),
            "darkgray" | "dark_gray" => Some(Self::DarkGray),
            "lightred" | "light_red" => Some(Self::LightRed),
            "lightgreen" | "light_green" => Some(Self::LightGreen),
            "lightyellow" | "light_yellow" => Some(Self::LightYellow),
            "lightblue" | "light_blue" => Some(Self::LightBlue),
            "lightmagenta" | "light_magenta" => Some(Self::LightMagenta),
            "lightcyan" | "light_cyan" => Some(Self::LightCyan),
            _ => None,
        }
    }
}

/// Visual style for a cell.
///
/// Every field defaults, so the `.cellar` writer can omit unset ones.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[derive(Default)]
#[serde(default)]
pub struct CellStyle {
    pub bold: bool,
    pub underline: bool,
    pub fg_color: Option<TerminalColor>,
    pub bg_color: Option<TerminalColor>,
}


/// Cell formatting options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CellFormat {
    /// Number format
    pub number_format: NumberFormat,
    /// Cell visual style
    pub style: CellStyle,
}

impl Default for CellFormat {
    fn default() -> Self {
        Self {
            number_format: NumberFormat::General,
            style: CellStyle::default(),
        }
    }
}

/// Formats a cell value according to the given number format.
pub fn format_cell_value(value: &str, format: &CellFormat) -> String {
    match &format.number_format {
        NumberFormat::General => value.to_string(),
        NumberFormat::Number { decimals, thousands_sep } => {
            if let Ok(n) = value.parse::<f64>() {
                let formatted = format!("{:.prec$}", n, prec = (*decimals).min(MAX_DECIMALS) as usize);
                if *thousands_sep {
                    add_thousands_separator(&formatted)
                } else {
                    formatted
                }
            } else {
                value.to_string()
            }
        }
        NumberFormat::Currency { symbol, decimals } => {
            if let Ok(n) = value.parse::<f64>() {
                // The sign goes before everything ("-$42.50", "-42.50 €"),
                // never between the symbol and the digits.
                let conv = CurrencyNotation::for_symbol(symbol);
                let abs_formatted = format!("{:.prec$}", n.abs(), prec = (*decimals).min(MAX_DECIMALS) as usize);
                let body = add_thousands_separator(&abs_formatted);
                let sign = if n < 0.0 && abs_formatted.bytes().any(|b| b.is_ascii_digit() && b != b'0') { "-" } else { "" };
                match (conv.symbol_after, conv.space) {
                    (true, true) => format!("{}{} {}", sign, body, symbol),
                    (true, false) => format!("{}{}{}", sign, body, symbol),
                    (false, true) => format!("{}{} {}", sign, symbol, body),
                    (false, false) => format!("{}{}{}", sign, symbol, body),
                }
            } else {
                value.to_string()
            }
        }
        NumberFormat::Percentage { decimals } => {
            if let Ok(n) = value.parse::<f64>() {
                format!("{:.prec$}%", n * 100.0, prec = (*decimals).min(MAX_DECIMALS) as usize)
            } else {
                value.to_string()
            }
        }
    }
}

/// Where a currency's symbol goes, as its home countries write it:
/// `1,234.56 €`, `$1,234.56`, `CHF 1,234.56`. Picked from the symbol, so a
/// file shows the same on every machine. The digits always use `.` for
/// decimals and `,` for thousands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurrencyNotation {
    pub symbol_after: bool,
    /// A space between the number and the symbol.
    pub space: bool,
}

impl CurrencyNotation {
    pub fn for_symbol(symbol: &str) -> Self {
        let n = |symbol_after, space| Self { symbol_after, space };
        match symbol.trim() {
            "€" | "EUR" | "kr" | "kr." | "SEK" | "NOK" | "DKK" | "zł" | "PLN" | "Kč" | "CZK" | "₽" | "RUB" | "₴"
            | "UAH" | "Ft" | "HUF" => n(true, true),
            // Letter codes read better with a space: "CHF 1,234.56".
            s if s.chars().last().is_some_and(|c| c.is_alphabetic()) => n(false, true),
            _ => n(false, false),
        }
    }
}

pub(super) fn add_thousands_separator(s: &str) -> String {
    let parts: Vec<&str> = s.splitn(2, '.').collect();
    let int_part = parts[0];
    let negative = int_part.starts_with('-');
    let digits = if negative { &int_part[1..] } else { int_part };

    let mut result = String::new();
    for (i, c) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    let int_formatted: String = result.chars().rev().collect();
    let prefix = if negative { "-" } else { "" };

    if parts.len() > 1 {
        format!("{}{}.{}", prefix, int_formatted, parts[1])
    } else {
        format!("{}{}", prefix, int_formatted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CellData};
    #[test]
    fn has_volatile_cf_predicate_detects_now_in_rule() {
        let mut sheet = Spreadsheet::default();
        // Pure predicate — not volatile.
        sheet.conditional_formats.push(ConditionalFormat {
            column: 0,
            predicate: "_ > 100".to_string(),
            style: CellStyle::default(),
        });
        assert!(!sheet.has_volatile_cf_predicate());
        // Add a NOW()-based rule — sheet is now volatile.
        sheet.conditional_formats.push(ConditionalFormat {
            column: 1,
            predicate: "NOW() > _".to_string(),
            style: CellStyle::default(),
        });
        assert!(sheet.has_volatile_cf_predicate());
    }

    #[test]
    fn recalc_clears_cf_cache_when_predicate_is_volatile() {
        use crate::domain::Workbook;
        let mut wb = Workbook::default();
        wb.sheets[0].set_cell(0, 0, CellData {
            value: "1".to_string(), formula: None, format: None, comment: None,
            spill_anchor: None,
        });
        wb.sheets[0].conditional_formats.push(ConditionalFormat {
            column: 0,
            predicate: "NOW() > _".to_string(),
            style: CellStyle { bold: true, ..CellStyle::default() },
        });
        // Prime the cache.
        let _ = wb.sheets[0].conditional_style_for(0, 0);
        assert!(!wb.sheets[0].cf_cache.lock().unwrap().is_empty(),
            "predicate eval should have populated the cache");
        // Force a recalc — even with no dirty cells, the post-recalc hook
        // must still clear the cache because the predicate is volatile.
        wb.mark_all_formula_cells_dirty();
        let _ = wb.recalc_via_graph_result();
        assert!(wb.sheets[0].cf_cache.lock().unwrap().is_empty(),
            "volatile CF predicate must invalidate cf_cache on recalc");
    }

    #[test]
    fn recalc_keeps_cf_cache_when_predicates_are_pure() {
        use crate::domain::Workbook;
        let mut wb = Workbook::default();
        wb.sheets[0].set_cell(0, 0, CellData {
            value: "150".to_string(), formula: None, format: None, comment: None,
            spill_anchor: None,
        });
        wb.sheets[0].conditional_formats.push(ConditionalFormat {
            column: 0,
            predicate: "_ > 100".to_string(),
            style: CellStyle { bold: true, ..CellStyle::default() },
        });
        // Prime the cache.
        let _ = wb.sheets[0].conditional_style_for(0, 0);
        assert!(!wb.sheets[0].cf_cache.lock().unwrap().is_empty());
        // Recalc with no dirty cells touching this sheet: cache stays
        // because all rules are pure. (A cell mutation would still
        // invalidate via `set_cell_internal`.)
        let _ = wb.recalc_via_graph_result();
        assert!(!wb.sheets[0].cf_cache.lock().unwrap().is_empty(),
            "pure CF predicates must not trigger spurious cache invalidation");
    }

    #[test]
    fn test_conditional_format_fires_on_truthy_predicate() {
        let mut sheet = Spreadsheet::default();
        sheet.set_cell(0, 0, CellData {
            value: "150".to_string(), formula: None, format: None, comment: None,
        spill_anchor: None,
        });
        sheet.set_cell(1, 0, CellData {
            value: "50".to_string(), formula: None, format: None, comment: None,
        spill_anchor: None,
        });
        sheet.conditional_formats.push(ConditionalFormat {
            column: 0,
            predicate: "_ > 100".to_string(),
            style: CellStyle {
                bold: true,
                underline: false,
                fg_color: Some(TerminalColor::Red),
                bg_color: None,
            },
        });
        let s0 = sheet.conditional_style_for(0, 0);
        assert!(s0.is_some());
        assert!(s0.as_ref().unwrap().bold);
        assert_eq!(s0.unwrap().fg_color, Some(TerminalColor::Red));
        // Row 1 doesn't satisfy the predicate.
        assert!(sheet.conditional_style_for(1, 0).is_none());
    }

    #[test]
    fn test_thousands_separator_edge_cases() {
        let fmt = CellFormat {
            number_format: NumberFormat::Number { decimals: 2, thousands_sep: true },
            style: CellStyle::default(),
        };
        assert_eq!(format_cell_value("1234567.89", &fmt), "1,234,567.89");
        assert_eq!(format_cell_value("-1234.5", &fmt), "-1,234.50");
        assert_eq!(format_cell_value("999.99", &fmt), "999.99");
        assert_eq!(format_cell_value("0", &fmt), "0.00");
        assert_eq!(format_cell_value("-0.5", &fmt), "-0.50");

        // Whole-million boundary
        let fmt0 = CellFormat {
            number_format: NumberFormat::Number { decimals: 0, thousands_sep: true },
            style: CellStyle::default(),
        };
        assert_eq!(format_cell_value("1000000", &fmt0), "1,000,000");
        assert_eq!(format_cell_value("-1000000", &fmt0), "-1,000,000");
    }

    #[test]
    fn test_format_cell_value_general() {
        let fmt = CellFormat { number_format: NumberFormat::General, ..CellFormat::default() };
        assert_eq!(super::format_cell_value("42.5", &fmt), "42.5");
        assert_eq!(super::format_cell_value("hello", &fmt), "hello");
    }

    #[test]
    fn test_format_cell_value_number() {
        let fmt = CellFormat { number_format: NumberFormat::Number { decimals: 2, thousands_sep: false }, ..CellFormat::default() };
        assert_eq!(super::format_cell_value("42", &fmt), "42.00");
        assert_eq!(super::format_cell_value("3.14159", &fmt), "3.14");
        assert_eq!(super::format_cell_value("hello", &fmt), "hello"); // non-numeric passthrough
    }

    #[test]
    fn test_format_cell_value_number_thousands() {
        let fmt = CellFormat { number_format: NumberFormat::Number { decimals: 2, thousands_sep: true }, ..CellFormat::default() };
        assert_eq!(super::format_cell_value("1234567.89", &fmt), "1,234,567.89");
        assert_eq!(super::format_cell_value("42", &fmt), "42.00");
    }

    #[test]
    fn test_format_cell_value_currency() {
        let fmt = CellFormat { number_format: NumberFormat::Currency { symbol: "$".to_string(), decimals: 2 }, ..CellFormat::default() };
        assert_eq!(super::format_cell_value("1234.5", &fmt), "$1,234.50");
        assert_eq!(super::format_cell_value("42", &fmt), "$42.00");
        assert_eq!(super::format_cell_value("hello", &fmt), "hello");
        assert_eq!(super::format_cell_value("-42.5", &fmt), "-$42.50");
    }

    #[test]
    fn currency_follows_its_own_notation() {
        let cur = |symbol: &str, decimals| CellFormat {
            number_format: NumberFormat::Currency { symbol: symbol.to_string(), decimals },
            ..CellFormat::default()
        };
        assert_eq!(format_cell_value("1234567.5", &cur("€", 2)), "1,234,567.50 €");
        assert_eq!(format_cell_value("-42", &cur("€", 2)), "-42.00 €");
        assert_eq!(format_cell_value("1234.6", &cur("€", 0)), "1,235 €");
        assert_eq!(format_cell_value("1234.5", &cur("£", 2)), "£1,234.50");
        assert_eq!(format_cell_value("1234.5", &cur("CHF", 2)), "CHF 1,234.50");
        assert_eq!(format_cell_value("1234.5", &cur("zł", 2)), "1,234.50 zł");
        // Rounds to zero: no "-0.00 €".
        assert_eq!(format_cell_value("-0.001", &cur("€", 2)), "0.00 €");
    }

    #[test]
    fn decimals_helpers() {
        assert_eq!(NumberFormat::General.with_decimals(3), NumberFormat::Number { decimals: 3, thousands_sep: false });
        let eur = NumberFormat::Currency { symbol: "€".into(), decimals: 2 };
        assert_eq!(eur.with_decimals(0), NumberFormat::Currency { symbol: "€".into(), decimals: 0 });
        assert_eq!(eur.decimals(), Some(2));
        assert_eq!(NumberFormat::Percentage { decimals: 1 }.with_decimals(99).decimals(), Some(MAX_DECIMALS));
    }

    #[test]
    fn test_format_cell_value_percentage() {
        let fmt = CellFormat { number_format: NumberFormat::Percentage { decimals: 1 }, ..CellFormat::default() };
        assert_eq!(super::format_cell_value("0.75", &fmt), "75.0%");
        assert_eq!(super::format_cell_value("1", &fmt), "100.0%");
        assert_eq!(super::format_cell_value("0.123", &fmt), "12.3%");
    }

    #[test]
    fn test_thousands_separator() {
        assert_eq!(super::style::add_thousands_separator("1234567"), "1,234,567");
        assert_eq!(super::style::add_thousands_separator("123"), "123");
        assert_eq!(super::style::add_thousands_separator("1234.56"), "1,234.56");
        assert_eq!(super::style::add_thousands_separator("-1234567"), "-1,234,567");
    }

    #[test]
    fn test_cell_data_format_serialization() {
        let cell = CellData {
            value: "100".to_string(),
            formula: None,
            format: Some(CellFormat {
                number_format: NumberFormat::Percentage { decimals: 1 },
                ..CellFormat::default()
            }),
            comment: None,
        spill_anchor: None,
        };
        let json = serde_json::to_string(&cell).unwrap();
        let deserialized: CellData = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.value, "100");
        assert!(deserialized.format.is_some());
        assert!(matches!(deserialized.format.unwrap().number_format, NumberFormat::Percentage { decimals: 1 }));
    }

}
