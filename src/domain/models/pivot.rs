//! Submodule of `models` — see models/mod.rs.
//!
//! PivotTable definitions, modelled on Excel's PivotTable: a source range
//! with a header row, fields placed in the Filters / Columns / Rows / Values
//! areas, per-field settings (item filter, sort, subtotals, top-N) and
//! per-value settings (summarize-by function, "Show Values As", number
//! format). Only the definition is stored; results are computed on demand
//! by `services::pivot`, so a data edit never touches the pivot's entry in
//! the saved file.

use serde::{Deserialize, Serialize};

use super::{is_default, NumberFormat};

fn yes() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}

/// Text Excel shows for an empty source value.
pub const BLANK_ITEM: &str = "(blank)";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PivotSpec {
    /// Display name, e.g. `PivotTable1`. PivotCharts refer to it.
    pub name: String,
    /// Source range including the header row, e.g. `A1:F200` or
    /// `'Raw data'!A1:F200`. Unqualified ranges are on the owning sheet.
    pub source: String,
    /// Report filters (Excel's "Filters" area).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<PivotField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<PivotField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<PivotField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<PivotValue>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub options: PivotOptions,
}

impl PivotSpec {
    pub fn new(name: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: source.into(),
            filters: Vec::new(),
            rows: Vec::new(),
            columns: Vec::new(),
            values: Vec::new(),
            options: PivotOptions::default(),
        }
    }

    /// Is `field` placed in Filters, Rows or Columns (or Values)?
    pub fn uses_field(&self, field: &str) -> bool {
        self.filters.iter().chain(&self.rows).chain(&self.columns).any(|f| f.field == field)
            || self.values.iter().any(|v| v.field == field)
    }
}

/// A field in the Filters, Rows or Columns area with its "Field Settings".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PivotField {
    /// Header text of the source column.
    pub field: String,
    /// Unchecked items in the field's filter list. Stored as the hidden set
    /// (like Excel) so new items in the data show up automatically.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_items: Vec<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub sort: PivotSort,
    /// Field Settings → Subtotals: Automatic (true) or None (false).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub subtotals: bool,
    /// Value Filters → Top 10…
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_filter: Option<TopFilter>,
}

impl PivotField {
    pub fn new(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            hidden_items: Vec::new(),
            sort: PivotSort::default(),
            subtotals: true,
            top_filter: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PivotSort {
    /// Sort A to Z (Excel's default for a new field).
    #[default]
    Ascending,
    /// Sort Z to A.
    Descending,
    /// Data source order.
    Manual,
    /// More Sort Options → by a value field.
    ByValue { value: usize, descending: bool },
}

/// Top/Bottom N items by a value field ("Top 10 Filter").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopFilter {
    pub top: bool,
    pub count: usize,
    /// Index into `PivotSpec::values`.
    pub by_value: usize,
}

/// A field in the Values area with its "Value Field Settings".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PivotValue {
    pub field: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub summarize: Summarize,
    /// Custom Name; `None` means Excel's default ("Sum of Revenue").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub show_as: ShowValuesAs,
    /// Base field for the "Show Values As" calculations that need one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_field: Option<String>,
    /// Base item: an item label, `(previous)` or `(next)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_item: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_format: Option<NumberFormat>,
}

impl PivotValue {
    pub fn new(field: impl Into<String>, summarize: Summarize) -> Self {
        Self {
            field: field.into(),
            summarize,
            name: None,
            show_as: ShowValuesAs::default(),
            base_field: None,
            base_item: None,
            number_format: None,
        }
    }

    pub fn display_name(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("{} of {}", self.summarize.label(), self.field))
    }
}

/// "Summarize value field by" — Excel's eleven functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Summarize {
    #[default]
    Sum,
    Count,
    Average,
    Max,
    Min,
    Product,
    CountNumbers,
    StdDev,
    StdDevP,
    Var,
    VarP,
}

impl Summarize {
    pub const ALL: [Summarize; 11] = [
        Self::Sum,
        Self::Count,
        Self::Average,
        Self::Max,
        Self::Min,
        Self::Product,
        Self::CountNumbers,
        Self::StdDev,
        Self::StdDevP,
        Self::Var,
        Self::VarP,
    ];

    /// The label Excel uses in the list and in default value names.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Sum => "Sum",
            Self::Count => "Count",
            Self::Average => "Average",
            Self::Max => "Max",
            Self::Min => "Min",
            Self::Product => "Product",
            Self::CountNumbers => "Count Numbers",
            Self::StdDev => "StdDev",
            Self::StdDevP => "StdDevp",
            Self::Var => "Var",
            Self::VarP => "Varp",
        }
    }
}

/// "Show Values As" — the calculations Excel offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ShowValuesAs {
    #[default]
    NoCalculation,
    PercentOfGrandTotal,
    PercentOfColumnTotal,
    PercentOfRowTotal,
    PercentOf,
    PercentOfParentRowTotal,
    PercentOfParentColumnTotal,
    PercentOfParentTotal,
    DifferenceFrom,
    PercentDifferenceFrom,
    RunningTotal,
    PercentRunningTotal,
    RankAscending,
    RankDescending,
    Index,
}

impl ShowValuesAs {
    pub const ALL: [ShowValuesAs; 15] = [
        Self::NoCalculation,
        Self::PercentOfGrandTotal,
        Self::PercentOfColumnTotal,
        Self::PercentOfRowTotal,
        Self::PercentOf,
        Self::PercentOfParentRowTotal,
        Self::PercentOfParentColumnTotal,
        Self::PercentOfParentTotal,
        Self::DifferenceFrom,
        Self::PercentDifferenceFrom,
        Self::RunningTotal,
        Self::PercentRunningTotal,
        Self::RankAscending,
        Self::RankDescending,
        Self::Index,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::NoCalculation => "No Calculation",
            Self::PercentOfGrandTotal => "% of Grand Total",
            Self::PercentOfColumnTotal => "% of Column Total",
            Self::PercentOfRowTotal => "% of Row Total",
            Self::PercentOf => "% Of",
            Self::PercentOfParentRowTotal => "% of Parent Row Total",
            Self::PercentOfParentColumnTotal => "% of Parent Column Total",
            Self::PercentOfParentTotal => "% of Parent Total",
            Self::DifferenceFrom => "Difference From",
            Self::PercentDifferenceFrom => "% Difference From",
            Self::RunningTotal => "Running Total In",
            Self::PercentRunningTotal => "% Running Total In",
            Self::RankAscending => "Rank Smallest to Largest",
            Self::RankDescending => "Rank Largest to Smallest",
            Self::Index => "Index",
        }
    }

    pub fn needs_base_field(&self) -> bool {
        matches!(
            self,
            Self::PercentOf
                | Self::PercentOfParentTotal
                | Self::DifferenceFrom
                | Self::PercentDifferenceFrom
                | Self::RunningTotal
                | Self::PercentRunningTotal
                | Self::RankAscending
                | Self::RankDescending
        )
    }

    pub fn needs_base_item(&self) -> bool {
        matches!(self, Self::PercentOf | Self::DifferenceFrom | Self::PercentDifferenceFrom)
    }

    /// Results that read naturally as percentages.
    pub fn is_percentage(&self) -> bool {
        matches!(
            self,
            Self::PercentOfGrandTotal
                | Self::PercentOfColumnTotal
                | Self::PercentOfRowTotal
                | Self::PercentOf
                | Self::PercentOfParentRowTotal
                | Self::PercentOfParentColumnTotal
                | Self::PercentOfParentTotal
                | Self::PercentDifferenceFrom
                | Self::PercentRunningTotal
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PivotLayout {
    #[default]
    Compact,
    Outline,
    Tabular,
}

/// PivotTable Options + the Design tab's layout switches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PivotOptions {
    pub layout: PivotLayout,
    /// "Show grand totals for rows" — the Grand Total *column*.
    pub grand_totals_rows: bool,
    /// "Show grand totals for columns" — the Grand Total *row*.
    pub grand_totals_columns: bool,
    /// Design → Subtotals → Show at Top (compact/outline layouts).
    pub subtotals_at_top: bool,
    /// Σ Values placed in Rows instead of Columns.
    pub values_on_rows: bool,
    /// Design → Report Layout → Repeat All Item Labels.
    pub repeat_item_labels: bool,
    /// "For empty cells show:".
    pub empty_cells: String,
}

impl Default for PivotOptions {
    fn default() -> Self {
        Self {
            layout: PivotLayout::Compact,
            grand_totals_rows: true,
            grand_totals_columns: true,
            subtotals_at_top: true,
            values_on_rows: false,
            repeat_item_labels: false,
            empty_cells: String::new(),
        }
    }
}
