//! Excel PivotTable → Cellar [`PivotSpec`].
//!
//! An Excel pivot is two parts: the `pivotTableDefinition` (which fields
//! are in which area, value-field settings, sorting, layout) and the
//! `pivotCacheDefinition` it points to (source range and field names, plus
//! the shared item values that hidden-item flags refer to by index).

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::domain::{
    NumberFormat, PivotField, PivotLayout, PivotSort, PivotSpec, PivotValue, ShowValuesAs, Summarize,
    BLANK_ITEM,
};

/// A converted pivot plus where Excel had rendered it (`A3:C7`).
#[derive(Debug, Clone)]
pub struct ImportedPivot {
    pub spec: PivotSpec,
    pub location: String,
}

#[derive(Default, Debug)]
struct CacheField {
    name: String,
    items: Vec<String>,
}

#[derive(Default, Debug)]
struct ExcelPivotField {
    sort: Option<String>,
    default_subtotal: bool,
    compact: Option<bool>,
    outline: Option<bool>,
    subtotal_top: Option<bool>,
    /// (shared item index, hidden)
    items: Vec<(usize, bool)>,
    /// Data field index this field is sorted by (autoSortScope).
    sort_by_data: Option<usize>,
}

/// Excel's magic `baseItem` values for "(previous)" / "(next)".
const BASE_PREVIOUS: u64 = 1_048_828;
const BASE_NEXT: u64 = 1_048_829;
/// `field x="-2"` / `4294967294`: the Σ Values pseudo-field.
const DATA_FIELD: i64 = -2;

fn attr(e: &BytesStart, key: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == key)
        .map(|a| super::xlsx_extras::unescape_pub(&a.value))
}

fn flag(e: &BytesStart, key: &[u8]) -> Option<bool> {
    attr(e, key).map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

fn num<T: std::str::FromStr>(e: &BytesStart, key: &[u8]) -> Option<T> {
    attr(e, key).and_then(|v| v.parse().ok())
}

/// `worksheetSource`: a sheet + range, or a defined/table name.
#[derive(Default)]
struct CacheSource {
    sheet: Option<String>,
    range: Option<String>,
    name: Option<String>,
}

/// A `dataField` as written in the definition.
struct RawDataField {
    field: usize,
    subtotal: String,
    name: Option<String>,
    show_as: String,
    base_field: Option<usize>,
    base_item: Option<u64>,
    num_fmt: Option<u32>,
}

/// Source and fields of a pivot cache.
fn parse_cache(xml: &str) -> Result<(CacheSource, Vec<CacheField>), String> {
    let mut reader = Reader::from_str(xml);
    let mut source = CacheSource::default();
    let mut fields: Vec<CacheField> = Vec::new();
    let mut in_shared = false;
    loop {
        let (e, empty) = match reader.read_event() {
            Ok(Event::Start(e)) => (e, false),
            Ok(Event::Empty(e)) => (e, true),
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == b"sharedItems" {
                    in_shared = false;
                }
                continue;
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("pivot cache: {}", e)),
            _ => continue,
        };
        match e.local_name().as_ref() {
            b"worksheetSource" => {
                source = CacheSource { sheet: attr(&e, b"sheet"), range: attr(&e, b"ref"), name: attr(&e, b"name") };
            }
            b"cacheField" => fields.push(CacheField { name: attr(&e, b"name").unwrap_or_default(), items: Vec::new() }),
            b"sharedItems" => in_shared = !empty,
            b"s" | b"n" | b"b" | b"e" | b"d" | b"m" if in_shared => {
                let v = attr(&e, b"v").unwrap_or_default();
                let label = match e.local_name().as_ref() {
                    b"m" => BLANK_ITEM.to_string(),
                    // Dates are stored as 2026-01-05T00:00:00; the importer
                    // shows them as 2026-01-05.
                    b"d" => v.strip_suffix("T00:00:00").unwrap_or(&v).to_string(),
                    b"b" => if v == "1" { "TRUE".into() } else { "FALSE".into() },
                    b"n" => v.parse::<f64>().map(|x| format!("{}", x)).unwrap_or(v),
                    _ => v,
                };
                if let Some(f) = fields.last_mut() {
                    f.items.push(label);
                }
            }
            _ => {}
        }
    }
    Ok((source, fields))
}

/// Convert one Excel pivot. `named_ranges` resolves name-based sources;
/// `host_sheet` is the sheet the pivot sits on.
pub fn parse_pivot(
    definition: &str,
    cache: &str,
    host_sheet: &str,
    named_ranges: &dyn Fn(&str) -> Option<String>,
) -> Result<ImportedPivot, String> {
    let (CacheSource { sheet: src_sheet, range: src_ref, name: src_name }, cache_fields) = parse_cache(cache)?;
    let source_range = match (&src_ref, &src_name) {
        (Some(r), _) => r.clone(),
        (None, Some(n)) => named_ranges(n).ok_or_else(|| format!("its source \"{}\" is a table or name Cellar doesn't know", n))?,
        _ => return Err("its source isn't a worksheet range (external data or OLAP)".into()),
    };
    let source = match src_sheet {
        Some(s) if s != host_sheet && !source_range.contains('!') => {
            if s.chars().all(|c| c.is_alphanumeric() || c == '_') { format!("{}!{}", s, source_range) } else { format!("'{}'!{}", s.replace('\'', "''"), source_range) }
        }
        _ => source_range,
    }
    .replace('$', "");

    let mut reader = Reader::from_str(definition);
    let mut spec = PivotSpec::new("PivotTable", source);
    let mut location = String::new();
    let mut table_compact = true;
    let mut table_outline = false;
    let mut pfields: Vec<ExcelPivotField> = Vec::new();
    let mut row_fields: Vec<i64> = Vec::new();
    let mut col_fields: Vec<i64> = Vec::new();
    let mut page_fields: Vec<(usize, Option<usize>)> = Vec::new();
    let mut data_fields: Vec<RawDataField> = Vec::new();
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut auto_sort_ref_is_data = false;
    loop {
        let (e, empty) = match reader.read_event() {
            Ok(Event::Start(e)) => (e, false),
            Ok(Event::Empty(e)) => (e, true),
            Ok(Event::End(_)) => {
                stack.pop();
                continue;
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("pivot definition: {}", e)),
            _ => continue,
        };
        let name = e.local_name().as_ref().to_vec();
        let parent = stack.last().map(|p| p.as_slice());
        let inside = |n: &[u8]| stack.iter().any(|s| s.as_slice() == n);
        match name.as_slice() {
            b"pivotTableDefinition" => {
                spec.name = attr(&e, b"name").unwrap_or_else(|| "PivotTable1".into());
                table_compact = flag(&e, b"compact").unwrap_or(true);
                table_outline = flag(&e, b"outline").unwrap_or(false);
                spec.options.grand_totals_rows = flag(&e, b"rowGrandTotals").unwrap_or(true);
                spec.options.grand_totals_columns = flag(&e, b"colGrandTotals").unwrap_or(true);
                spec.options.values_on_rows = flag(&e, b"dataOnRows").unwrap_or(false);
                if flag(&e, b"showMissing").unwrap_or(true)
                    && let Some(caption) = attr(&e, b"missingCaption")
                {
                    spec.options.empty_cells = caption;
                }
            }
            b"location" => location = attr(&e, b"ref").unwrap_or_default(),
            b"pivotField" if parent == Some(b"pivotFields") => pfields.push(ExcelPivotField {
                sort: attr(&e, b"sortType"),
                default_subtotal: flag(&e, b"defaultSubtotal").unwrap_or(true),
                compact: flag(&e, b"compact"),
                outline: flag(&e, b"outline"),
                subtotal_top: flag(&e, b"subtotalTop"),
                ..Default::default()
            }),
            b"item" if parent == Some(b"items") && inside(b"pivotField") => {
                if let (Some(f), Some(x)) = (pfields.last_mut(), num::<usize>(&e, b"x")) {
                    f.items.push((x, flag(&e, b"h").unwrap_or(false)));
                }
            }
            b"reference" if inside(b"autoSortScope") => {
                auto_sort_ref_is_data = attr(&e, b"field").as_deref() == Some("4294967294");
            }
            b"x" if inside(b"autoSortScope") && auto_sort_ref_is_data => {
                if let (Some(f), Some(v)) = (pfields.last_mut(), num::<usize>(&e, b"v")) {
                    f.sort_by_data = Some(v);
                }
            }
            b"field" if parent == Some(b"rowFields") => row_fields.push(num(&e, b"x").unwrap_or(0)),
            b"field" if parent == Some(b"colFields") => col_fields.push(num(&e, b"x").unwrap_or(0)),
            b"pageField" => {
                if let Some(fld) = num::<usize>(&e, b"fld") {
                    page_fields.push((fld, num(&e, b"item")));
                }
            }
            b"dataField" if parent == Some(b"dataFields") => {
                let fld: usize = num(&e, b"fld").ok_or("a value field has no source field")?;
                data_fields.push(RawDataField {
                    field: fld,
                    subtotal: attr(&e, b"subtotal").unwrap_or_else(|| "sum".into()),
                    name: attr(&e, b"name"),
                    show_as: attr(&e, b"showDataAs").unwrap_or_else(|| "normal".into()),
                    base_field: num(&e, b"baseField"),
                    base_item: num(&e, b"baseItem"),
                    num_fmt: num(&e, b"numFmtId"),
                });
            }
            // Excel 2010 extension carrying the newer "Show Values As" kinds.
            b"dataField" if inside(b"dataField") => {
                if let (Some(d), Some(kind)) = (data_fields.last_mut(), attr(&e, b"pivotShowAs")) {
                    d.show_as = kind;
                }
            }
            _ => {}
        }
        if !empty {
            stack.push(name);
        }
    }

    let field_name = |i: usize| -> Result<String, String> {
        cache_fields.get(i).map(|f| f.name.clone()).ok_or_else(|| format!("unknown field #{}", i))
    };
    let item_label = |field: usize, x: usize| cache_fields.get(field).and_then(|f| f.items.get(x)).cloned();
    let axis_field = |i: usize| -> Result<PivotField, String> {
        let mut f = PivotField::new(field_name(i)?);
        if let Some(pf) = pfields.get(i) {
            f.subtotals = pf.default_subtotal;
            f.hidden_items = pf.items.iter().filter(|(_, h)| *h).filter_map(|(x, _)| item_label(i, *x)).collect();
            f.sort = match (pf.sort.as_deref(), pf.sort_by_data) {
                (Some("descending"), Some(v)) => PivotSort::ByValue { value: v, descending: true },
                (Some("ascending"), Some(v)) => PivotSort::ByValue { value: v, descending: false },
                (Some("descending"), None) => PivotSort::Descending,
                // Excel's "manual" order is the items list, which it keeps
                // sorted A→Z unless the user dragged items around.
                _ => PivotSort::Ascending,
            };
        }
        Ok(f)
    };

    for &x in &row_fields {
        if x == DATA_FIELD {
            spec.options.values_on_rows = true;
        } else if x >= 0 {
            spec.rows.push(axis_field(x as usize)?);
        }
    }
    for &x in &col_fields {
        if x >= 0 {
            spec.columns.push(axis_field(x as usize)?);
        }
    }
    for &(fld, item) in &page_fields {
        let mut f = axis_field(fld)?;
        // A single selected item: every other item is hidden.
        if let Some(sel) = item.and_then(|x| item_label(fld, x)) {
            f.hidden_items = cache_fields[fld].items.iter().filter(|it| **it != sel).cloned().collect();
        }
        spec.filters.push(f);
    }
    for RawDataField { field: fld, subtotal, name, show_as, base_field, base_item, num_fmt: fmt } in data_fields {
        let summarize = match subtotal.as_str() {
            "count" => Summarize::Count,
            "average" => Summarize::Average,
            "max" => Summarize::Max,
            "min" => Summarize::Min,
            "product" => Summarize::Product,
            "countNums" => Summarize::CountNumbers,
            "stdDev" => Summarize::StdDev,
            "stdDevp" => Summarize::StdDevP,
            "var" => Summarize::Var,
            "varp" => Summarize::VarP,
            _ => Summarize::Sum,
        };
        let mut v = PivotValue::new(field_name(fld)?, summarize);
        if name.as_ref().is_some_and(|n| *n != v.display_name()) {
            v.name = name;
        }
        v.show_as = match show_as.as_str() {
            "difference" => ShowValuesAs::DifferenceFrom,
            "percent" => ShowValuesAs::PercentOf,
            "percentDiff" => ShowValuesAs::PercentDifferenceFrom,
            "runTotal" => ShowValuesAs::RunningTotal,
            "percentOfRow" => ShowValuesAs::PercentOfRowTotal,
            "percentOfCol" => ShowValuesAs::PercentOfColumnTotal,
            "percentOfTotal" => ShowValuesAs::PercentOfGrandTotal,
            "index" => ShowValuesAs::Index,
            "percentOfParentRow" => ShowValuesAs::PercentOfParentRowTotal,
            "percentOfParentCol" => ShowValuesAs::PercentOfParentColumnTotal,
            "percentOfParent" => ShowValuesAs::PercentOfParentTotal,
            "percentOfRunningTotal" => ShowValuesAs::PercentRunningTotal,
            "rankAscending" => ShowValuesAs::RankAscending,
            "rankDescending" => ShowValuesAs::RankDescending,
            _ => ShowValuesAs::NoCalculation,
        };
        if v.show_as.needs_base_field() {
            v.base_field = base_field.and_then(|b| field_name(b).ok());
        }
        if v.show_as.needs_base_item() {
            v.base_item = match base_item {
                Some(BASE_PREVIOUS) => Some("(previous)".into()),
                Some(BASE_NEXT) => Some("(next)".into()),
                Some(x) => base_field.and_then(|b| item_label(b, x as usize)),
                None => None,
            };
        }
        v.number_format = fmt.and_then(builtin_number_format);
        spec.values.push(v);
    }

    // Layout: per-field flags override the table defaults (Excel writes
    // them on each field); the first row field decides.
    let first = row_fields.iter().find(|x| **x >= 0).and_then(|x| pfields.get(*x as usize));
    let compact = first.and_then(|f| f.compact).unwrap_or(table_compact);
    let outline = first.and_then(|f| f.outline).unwrap_or(table_outline || compact);
    spec.options.layout = match (compact, outline) {
        (true, true) => PivotLayout::Compact,
        (false, true) => PivotLayout::Outline,
        _ => PivotLayout::Tabular,
    };
    spec.options.subtotals_at_top = first.and_then(|f| f.subtotal_top).unwrap_or(true);
    Ok(ImportedPivot { spec, location })
}

/// Number formats Excel stores by built-in id on value fields.
fn builtin_number_format(id: u32) -> Option<NumberFormat> {
    match id {
        1 => Some(NumberFormat::Number { decimals: 0, thousands_sep: false }),
        2 => Some(NumberFormat::Number { decimals: 2, thousands_sep: false }),
        3 => Some(NumberFormat::Number { decimals: 0, thousands_sep: true }),
        4 => Some(NumberFormat::Number { decimals: 2, thousands_sep: true }),
        9 => Some(NumberFormat::Percentage { decimals: 0 }),
        10 => Some(NumberFormat::Percentage { decimals: 2 }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CACHE: &str = r#"<pivotCacheDefinition xmlns="x">
      <cacheSource type="worksheet"><worksheetSource ref="$A$1:$D$7" sheet="Data"/></cacheSource>
      <cacheFields count="4">
        <cacheField name="Region"><sharedItems count="3"><s v="North"/><s v="South"/><s v="West"/></sharedItems></cacheField>
        <cacheField name="Month"><sharedItems count="2"><s v="Jan"/><s v="Feb"/></sharedItems></cacheField>
        <cacheField name="Revenue"><sharedItems containsNumber="1"/></cacheField>
        <cacheField name="Year"><sharedItems count="2"><n v="2025"/><n v="2026"/></sharedItems></cacheField>
      </cacheFields></pivotCacheDefinition>"#;

    const DEF: &str = r#"<pivotTableDefinition xmlns="x" xmlns:x14="y" name="Sales by Region" cacheId="1" colGrandTotals="0" outline="1" outlineData="1">
      <location ref="A3:D8" firstHeaderRow="1" firstDataRow="2" firstDataCol="1" rowPageCount="1" colPageCount="1"/>
      <pivotFields count="4">
        <pivotField axis="axisRow" showAll="0" sortType="descending">
          <items count="4"><item x="0"/><item x="1"/><item h="1" x="2"/><item t="default"/></items>
          <autoSortScope><pivotArea dataOnly="0" outline="0" fieldPosition="0"><references count="1">
            <reference field="4294967294" count="1" selected="0"><x v="0"/></reference></references></pivotArea></autoSortScope>
        </pivotField>
        <pivotField axis="axisCol" showAll="0" defaultSubtotal="0"><items count="2"><item x="0"/><item x="1"/></items></pivotField>
        <pivotField dataField="1" showAll="0"/>
        <pivotField axis="axisPage" showAll="0"><items count="3"><item x="0"/><item x="1"/><item t="default"/></items></pivotField>
      </pivotFields>
      <rowFields count="1"><field x="0"/></rowFields>
      <colFields count="2"><field x="1"/><field x="-2"/></colFields>
      <pageFields count="1"><pageField fld="3" item="1" hier="-1"/></pageFields>
      <dataFields count="2">
        <dataField name="Sum of Revenue" fld="2" baseField="0" baseItem="0" numFmtId="4"/>
        <dataField name="Share" fld="2" showDataAs="percentOfTotal" baseField="0" baseItem="0">
          <extLst><ext uri="{E15A36E0}"><x14:dataField pivotShowAs="percentOfParentRow"/></ext></extLst>
        </dataField>
      </dataFields>
    </pivotTableDefinition>"#;

    #[test]
    fn converts_fields_filters_values_and_layout() {
        let p = parse_pivot(DEF, CACHE, "Report", &|_| None).unwrap();
        let s = &p.spec;
        assert_eq!(s.name, "Sales by Region");
        assert_eq!(s.source, "Data!A1:D7");
        assert_eq!(p.location, "A3:D8");
        assert_eq!(s.rows.len(), 1);
        assert_eq!(s.rows[0].field, "Region");
        assert_eq!(s.rows[0].hidden_items, vec!["West"]);
        assert_eq!(s.rows[0].sort, PivotSort::ByValue { value: 0, descending: true });
        assert_eq!(s.columns[0].field, "Month");
        assert!(!s.columns[0].subtotals);
        assert_eq!(s.filters[0].field, "Year");
        assert_eq!(s.filters[0].hidden_items, vec!["2025"], "only 2026 selected");
        assert_eq!(s.values.len(), 2);
        assert_eq!(s.values[0].name, None, "default name is not stored");
        assert_eq!(s.values[0].number_format, Some(NumberFormat::Number { decimals: 2, thousands_sep: true }));
        assert_eq!(s.values[1].name.as_deref(), Some("Share"));
        assert_eq!(s.values[1].show_as, ShowValuesAs::PercentOfParentRowTotal);
        assert!(!s.options.grand_totals_columns);
        assert!(s.options.grand_totals_rows);
        assert_eq!(s.options.layout, PivotLayout::Compact);
    }

    #[test]
    fn tabular_layout_and_named_source() {
        let def = r#"<pivotTableDefinition name="P" compact="0" outline="0"><location ref="A1:B4"/>
          <pivotFields count="1"><pivotField axis="axisRow" compact="0" outline="0"/></pivotFields>
          <rowFields count="1"><field x="0"/></rowFields></pivotTableDefinition>"#;
        let cache = r#"<pivotCacheDefinition><cacheSource type="worksheet"><worksheetSource name="SalesData"/></cacheSource>
          <cacheFields count="1"><cacheField name="Region"/></cacheFields></pivotCacheDefinition>"#;
        let p = parse_pivot(def, cache, "S", &|n| (n == "SalesData").then(|| "A1:C9".to_string())).unwrap();
        assert_eq!(p.spec.source, "A1:C9");
        assert_eq!(p.spec.options.layout, PivotLayout::Tabular);
        assert!(parse_pivot(def, cache, "S", &|_| None).is_err());
    }
}
