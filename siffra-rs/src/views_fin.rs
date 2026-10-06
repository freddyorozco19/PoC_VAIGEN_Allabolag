//! Análisis financiero de una empresa real: gráficos SVG, ratios, cuenta de resultados, balance, personas y datos
//! completos del informe. Todo sale de los hechos que se guardan de cada informe anual.

use maud::{html, Markup};

use crate::annual_report::{FinancialYear, Financials, RawFact};
use crate::bolagsverket::Organisation;
use crate::db::ReportInfo;
use crate::scb::SectorMedians;
use crate::views::{icon_sized, info_btn, term, Ctx};

// ───────────────────────── Series y formato ─────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Unit {
    /// Miles de coronas.
    Tkr,
    Percent,
    /// Veces (ratio sin unidad).
    Times,
}

struct Series {
    name: String,
    values: Vec<Option<f64>>,
    /// Clase de color: `s1`..`s5`.
    class: &'static str,
}

fn years_of(fin: &Financials) -> Vec<&FinancialYear> {
    fin.years.iter().collect()
}

fn fmt_value(c: &Ctx, v: f64, unit: Unit) -> String {
    match unit {
        Unit::Tkr => format!("{} {}", c.int(v.round() as i64), c.t("unit.tkr")),
        Unit::Percent => c.pct1(v),
        Unit::Times => format!("{}×", c.dec(v, 1)),
    }
}

fn fmt_axis(c: &Ctx, v: f64, unit: Unit, millions: bool) -> String {
    match unit {
        Unit::Tkr if millions => format!("{} {}", c.num(v / 1000.0), c.t("unit.mkr")),
        Unit::Tkr => c.num(v),
        Unit::Percent => format!("{}%", c.num(v)),
        Unit::Times => c.num(v),
    }
}

/// Paso "redondo" (1, 2, 2.5, 5 × 10^k) mayor o igual que `x`.
fn nice_step(x: f64) -> f64 {
    if x <= 0.0 {
        return 1.0;
    }
    let pow = 10f64.powf(x.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * pow).find(|s| *s >= x * 0.999_999).unwrap_or(10.0 * pow)
}

/// Rango del eje con cuatro intervalos de cifras limpias. `include_zero`: las barras siempre arrancan en cero.
fn axis_range(values: impl Iterator<Item = f64>, include_zero: bool) -> (f64, f64) {
    let (mut lo, mut hi) = values.fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(v), h.max(v)));
    if lo > hi {
        return (0.0, 4.0);
    }
    if include_zero {
        lo = lo.min(0.0);
        hi = hi.max(0.0);
    }
    if (hi - lo).abs() < 1e-9 {
        hi = lo + 4.0;
    }
    let step = nice_step((hi - lo) / 4.0);
    ((lo / step).floor() * step, (hi / step).ceil() * step)
}

fn legend(items: &[(&str, &str)]) -> Markup {
    html! {
        div.legend {
            @for (name, class) in items { span.legend-item { span class={"swatch " (class)} {} (name) } }
        }
    }
}

/// Tabla con los mismos valores que el gráfico, para quien no puede o no quiere ver el SVG.
fn data_table(c: &Ctx, caption: &str, labels: &[String], series: &[Series], unit: Unit) -> Markup {
    html! {
        details.chart-data {
            summary { (c.t("chart.show_table")) }
            div.table-wrap.flat {
                table {
                    caption.sr-only { (caption) }
                    thead { tr { th scope="col" {} @for l in labels { th.right scope="col" { (l) } } } }
                    tbody {
                        @for s in series {
                            tr {
                                th scope="row" { (s.name) }
                                @for v in &s.values {
                                    td.right.num.mono { @match v { Some(x) => { (fmt_value(c, *x, unit)) } None => { span.muted { "—" } } } }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn point_tip(c: &Ctx, label: &str, series: &[Series], idx: usize, unit: Unit) -> String {
    let parts: Vec<String> = series.iter().filter_map(|s| s.values[idx].map(|v| format!("{}: {}", s.name, fmt_value(c, v, unit)))).collect();
    format!("{label} · {}", parts.join(" · "))
}

// ───────────────────────── Gráficos ─────────────────────────

const W: f64 = 420.0;
const H: f64 = 240.0;
const PL: f64 = 58.0;
const PR: f64 = 10.0;
const PT: f64 = 16.0;
const PB: f64 = 28.0;

fn grid(c: &Ctx, lo: f64, hi: f64, unit: Unit, millions: bool) -> Markup {
    let y = |v: f64| H - PB - (H - PB - PT) * (v - lo) / (hi - lo);
    html! {
        @for i in 0..5 {
            @let v = lo + (hi - lo) / 4.0 * i as f64;
            g {
                line x1=(PL) x2=(W - PR) y1=(y(v)) y2=(y(v)) stroke=(if v.abs() < 1e-9 { "var(--ink2)" } else { "var(--line)" }) stroke-width="1" {}
                text x=(PL - 8.0) y=(y(v) + 4.0) text-anchor="end" { (fmt_axis(c, v, unit, millions)) }
            }
        }
    }
}

/// Barras agrupadas (una por serie y año) con eje en cero, también para valores negativos.
fn grouped_bars(c: &Ctx, id: &str, title: &str, labels: &[String], series: &[Series], unit: Unit) -> Markup {
    let n = labels.len();
    if n == 0 || series.is_empty() {
        return html! {};
    }
    let all = series.iter().flat_map(|s| s.values.iter().flatten().copied());
    let (lo, hi) = axis_range(all, true);
    let millions = unit == Unit::Tkr && lo.abs().max(hi.abs()) >= 5000.0;
    let y = |v: f64| H - PB - (H - PB - PT) * (v - lo) / (hi - lo);
    let slot = (W - PL - PR) / n as f64;
    let group_w = slot * 0.72;
    let bar_w = group_w / series.len() as f64;
    let desc = (0..n).map(|i| point_tip(c, &labels[i], series, i, unit)).collect::<Vec<_>>().join(". ");
    html! {
        svg.chart viewBox=(format!("0 0 {W} {H}")) role="group" aria-labelledby=(format!("{id}-t {id}-d")) {
            title id=(format!("{id}-t")) { (title) }
            desc id=(format!("{id}-d")) { (desc) }
            (grid(c, lo, hi, unit, millions))
            @for i in 0..n {
                @let x0 = PL + i as f64 * slot + (slot - group_w) / 2.0;
                @let tip = point_tip(c, &labels[i], series, i, unit);
                g.bar-group tabindex="0" role="img" aria-label=(tip) data-tip=(tip) {
                    rect.hit x=(PL + i as f64 * slot) y=(PT) width=(slot) height=(H - PB - PT) fill="transparent" {}
                    @for (j, s) in series.iter().enumerate() {
                        @if let Some(v) = s.values[i] {
                            @let (top, bottom) = (y(v.max(0.0)), y(v.min(0.0)));
                            rect class={"bar " (s.class)} x=(x0 + j as f64 * bar_w + 1.0) y=(top) width=((bar_w - 2.0).max(2.0)) height=((bottom - top).max(1.0)) rx="2" {}
                        }
                    }
                    text x=(PL + i as f64 * slot + slot / 2.0) y=(H - 9.0) text-anchor="middle" { (labels[i]) }
                }
            }
        }
    }
}

/// Líneas (una por serie) con huecos donde falta el dato y una línea de referencia opcional (la mediana del sector).
fn line_chart(c: &Ctx, id: &str, title: &str, labels: &[String], series: &[Series], unit: Unit, reference: Option<(f64, String)>) -> Markup {
    let n = labels.len();
    if n == 0 || series.is_empty() {
        return html! {};
    }
    let all = series.iter().flat_map(|s| s.values.iter().flatten().copied()).chain(reference.iter().map(|(v, _)| *v));
    let (lo, hi) = axis_range(all, matches!(unit, Unit::Percent | Unit::Times));
    let y = |v: f64| H - PB - (H - PB - PT) * (v - lo) / (hi - lo);
    let slot = (W - PL - PR) / n as f64;
    let x = |i: usize| PL + i as f64 * slot + slot / 2.0;
    let desc = (0..n).map(|i| point_tip(c, &labels[i], series, i, unit)).collect::<Vec<_>>().join(". ");
    // Un segmento continuo por cada tramo de años consecutivos con dato.
    let paths: Vec<(&'static str, String)> = series
        .iter()
        .map(|s| {
            let mut d = String::new();
            let mut pen_down = false;
            for (i, v) in s.values.iter().enumerate() {
                match v {
                    Some(v) => {
                        d.push_str(&format!("{}{:.1},{:.1} ", if pen_down { "L" } else { "M" }, x(i), y(*v)));
                        pen_down = true;
                    }
                    None => pen_down = false,
                }
            }
            (s.class, d)
        })
        .collect();
    html! {
        svg.chart viewBox=(format!("0 0 {W} {H}")) role="group" aria-labelledby=(format!("{id}-t {id}-d")) {
            title id=(format!("{id}-t")) { (title) }
            desc id=(format!("{id}-d")) { (desc) }
            (grid(c, lo, hi, unit, false))
            @if let Some((v, label)) = &reference {
                g.reference-group tabindex="0" role="img" aria-label=(label) data-tip=(label) {
                    line.reference x1=(PL) x2=(W - PR) y1=(y(*v)) y2=(y(*v)) {}
                    rect x=(PL) y=(y(*v) - 6.0) width=(W - PL - PR) height="12" fill="transparent" {}
                }
            }
            @for (class, d) in &paths { path class={"line " (class)} d=(d.trim()) fill="none" {} }
            @for i in 0..n {
                @let tip = point_tip(c, &labels[i], series, i, unit);
                g.bar-group tabindex="0" role="img" aria-label=(tip) data-tip=(tip) {
                    rect.hit x=(PL + i as f64 * slot) y=(PT) width=(slot) height=(H - PB - PT) fill="transparent" {}
                    @for s in series { @if let Some(v) = s.values[i] { circle class={"dot " (s.class)} cx=(x(i)) cy=(y(v)) r="3.5" {} } }
                    text x=(x(i)) y=(H - 9.0) text-anchor="middle" { (labels[i]) }
                }
            }
        }
    }
}

/// Barras apiladas: cada año suma sus componentes (valores no negativos).
fn stacked_bars(c: &Ctx, id: &str, title: &str, labels: &[String], series: &[Series], unit: Unit) -> Markup {
    let n = labels.len();
    if n == 0 || series.is_empty() {
        return html! {};
    }
    let totals: Vec<f64> = (0..n).map(|i| series.iter().filter_map(|s| s.values[i]).map(|v| v.max(0.0)).sum()).collect();
    let (_, hi) = axis_range(totals.iter().copied(), true);
    let millions = unit == Unit::Tkr && hi >= 5000.0;
    let y = |v: f64| H - PB - (H - PB - PT) * v / hi;
    let slot = (W - PL - PR) / n as f64;
    let bar_w = slot * 0.6;
    let desc = (0..n).map(|i| point_tip(c, &labels[i], series, i, unit)).collect::<Vec<_>>().join(". ");
    // Para cada año, los tramos apilados (borde superior, alto, clase) calculados antes de dibujar.
    let stacks: Vec<Vec<(f64, f64, &'static str)>> = (0..n)
        .map(|i| {
            let mut acc = 0.0;
            series
                .iter()
                .filter_map(|s| {
                    let v = s.values[i].unwrap_or(0.0).max(0.0);
                    let seg = (v > 0.0).then(|| (y(acc + v), (y(acc) - y(acc + v)).max(0.5), s.class));
                    acc += v;
                    seg
                })
                .collect()
        })
        .collect();
    html! {
        svg.chart viewBox=(format!("0 0 {W} {H}")) role="group" aria-labelledby=(format!("{id}-t {id}-d")) {
            title id=(format!("{id}-t")) { (title) }
            desc id=(format!("{id}-d")) { (desc) }
            (grid(c, 0.0, hi, unit, millions))
            @for i in 0..n {
                @let x0 = PL + i as f64 * slot + (slot - bar_w) / 2.0;
                @let tip = point_tip(c, &labels[i], series, i, unit);
                g.bar-group tabindex="0" role="img" aria-label=(tip) data-tip=(tip) {
                    rect.hit x=(PL + i as f64 * slot) y=(PT) width=(slot) height=(H - PB - PT) fill="transparent" {}
                    @for (top, height, class) in &stacks[i] { rect class={"bar " (class)} x=(x0) y=(top) width=(bar_w) height=(height) {} }
                    text x=(PL + i as f64 * slot + slot / 2.0) y=(H - 9.0) text-anchor="middle" { (labels[i]) }
                }
            }
        }
    }
}

fn chart_card(title: &str, tip: &str, body: Markup, legend_items: Markup, table: Markup) -> Markup {
    html! {
        div.card {
            h2.card-title { (title) (info_btn(title, tip)) }
            (body)
            (legend_items)
            (table)
        }
    }
}

// ───────────────────────── Pestaña Finanzas ─────────────────────────

fn opt(v: Option<i64>) -> Option<f64> {
    v.map(|x| x as f64)
}

fn year_labels(fin: &Financials) -> Vec<String> {
    fin.years.iter().map(|y| y.label.clone()).collect()
}

fn sector_reference(c: &Ctx, m: Option<&SectorMedians>, pick: impl Fn(&SectorMedians) -> f64, unit: Unit) -> Option<(f64, String)> {
    m.map(|m| {
        let v = pick(m);
        (v, c.tf("fin.sector_line", &[&fmt_value(c, v, unit), &m.year]))
    })
}

fn ratio_row(c: &Ctx, label: Markup, years: &[&FinancialYear], f: impl Fn(&FinancialYear) -> Option<f64>, unit: Unit) -> Markup {
    html! {
        tr {
            th scope="row" { (label) }
            @for y in years {
                @match f(y) {
                    Some(v) => { td.right.num.mono { (fmt_value(c, v, unit)) } }
                    None => { td.right.num.mono.muted { "—" } }
                }
            }
        }
    }
}

fn money_row(c: &Ctx, label: &str, years: &[&FinancialYear], f: impl Fn(&FinancialYear) -> Option<i64>, strong: bool) -> Markup {
    html! {
        tr.strong[strong] {
            th scope="row" { (label) }
            @for y in years {
                @match f(y) {
                    Some(v) => { td.right.num.mono.neg[v < 0] { (c.int(v)) } }
                    None => { td.right.num.mono.muted { "—" } }
                }
            }
        }
    }
}

fn statement_table(c: &Ctx, caption: &str, years: &[&FinancialYear], rows: Markup) -> Markup {
    html! {
        div.table-wrap.flat {
            table.fin-table {
                caption.sr-only { (caption) }
                thead { tr { th scope="col" { (c.t("unit.tkr")) } @for y in years { th.right scope="col" { (y.label) } } } }
                tbody { (rows) }
            }
        }
    }
}

/// Empresas reales con cuentas digitales completas, para probar las pestañas desde el estado vacío.
pub const EXAMPLES_WITH_ACCOUNTS: [&str; 4] = ["5593006280", "5569705329", "5591106074", "5565877759"];

/// Estado vacío cuando Bolagsverket no tiene un informe digital: por qué, qué sí hay y empresas con datos completos.
pub fn no_accounts_card(c: &Ctx, org: &Organisation) -> Markup {
    html! {
        div.card.mt-4 {
            p.muted { (c.t("bok.none")) }
            p.note { (c.t("bok.none_note")) }
            p.note { (c.t("bok.none_why")) }
            p.note { a href=(format!("/foretag/{}", org.organisationsnummer)) { (c.t("bok.none_registry")) } }
            h3.card-subtitle { (c.t("bok.none_examples")) }
            div.chips.chips-left {
                @for n in EXAMPLES_WITH_ACCOUNTS {
                    @if n != org.organisationsnummer {
                        a.chip href=(format!("/foretag/{n}")) { (crate::views::format_orgnr(n)) }
                    }
                }
            }
        }
    }
}

/// Pestaña "Finanzas": gráficos, ratios, cuenta de resultados y balance.
pub fn finance_tab(c: &Ctx, org: &Organisation, fin: Option<&Financials>, medians: Option<&SectorMedians>) -> Markup {
    let Some(fin) = fin.filter(|f| f.latest().is_some()) else {
        return no_accounts_card(c, org);
    };
    let ys = years_of(fin);
    let labels = year_labels(fin);
    let col = |f: &dyn Fn(&FinancialYear) -> Option<f64>| -> Vec<Option<f64>> { ys.iter().map(|y| f(y)).collect() };

    // Facturación y resultados
    let money_series = vec![
        Series { name: c.t("fin.revenue").to_string(), values: col(&|y| opt(y.revenue)), class: "s1" },
        Series { name: c.t("fin.operating_result").to_string(), values: col(&|y| opt(y.operating_result)), class: "s2" },
        Series { name: c.t("fin.result_year").to_string(), values: col(&|y| opt(y.net_result.or(y.result))), class: "s3" },
    ];
    // Márgenes
    let margin_series = vec![
        Series { name: c.t("fin.operating_margin").to_string(), values: col(&|y| y.operating_margin()), class: "s2" },
        Series { name: c.t("term.margin").to_string(), values: col(&|y| y.margin()), class: "s1" },
        Series { name: c.t("fin.net_margin").to_string(), values: col(&|y| y.net_margin()), class: "s3" },
    ];
    let solidity_series = vec![Series { name: c.t("term.solidity").to_string(), values: col(&|y| y.solidity()), class: "s1" }];
    let liquidity_series = vec![
        Series { name: c.t("term.liquidity").to_string(), values: col(&|y| y.liquidity()), class: "s1" },
        Series { name: c.t("fin.current_ratio").to_string(), values: col(&|y| y.current_ratio()), class: "s4" },
    ];
    // Estructura de la financiación: patrimonio neto, deuda a largo plazo, deuda a corto plazo y otros
    let other = |y: &FinancialYear| -> Option<f64> {
        let known = y.equity? + y.long_term_debt.unwrap_or(0) + y.short_term_debt.unwrap_or(0);
        let rest = y.assets? - known;
        (y.assets.is_some() && rest > 0).then_some(rest as f64)
    };
    let structure_series = vec![
        Series { name: c.t("fin.equity").to_string(), values: col(&|y| opt(y.equity)), class: "s1" },
        Series { name: c.t("fin.long_term_debt").to_string(), values: col(&|y| opt(y.long_term_debt)), class: "s2" },
        Series { name: c.t("fin.short_term_debt").to_string(), values: col(&|y| opt(y.short_term_debt)), class: "s3" },
        Series { name: c.t("fin.other_liabilities").to_string(), values: col(&|y| other(y)), class: "s5" },
    ];
    let has = |s: &Series| s.values.iter().any(|v| v.is_some());
    let structure_ok = structure_series.iter().filter(|s| has(s)).count() >= 2;

    let latest = fin.latest().expect("comprobado arriba");
    // Reparto de los costes del último ejercicio sobre la facturación
    let cost_parts: Vec<(String, Option<i64>, &'static str)> = vec![
        (c.t("fin.personnel_cost").to_string(), latest.personnel_cost, "s1"),
        (c.t("fin.goods_cost").to_string(), latest.goods_cost, "s2"),
        (c.t("fin.other_external").to_string(), latest.other_external, "s3"),
        (c.t("fin.depreciation").to_string(), latest.depreciation, "s4"),
    ];
    let known_costs: i64 = cost_parts.iter().filter_map(|(_, v, _)| *v).sum();
    let rest_costs = latest.operating_costs.map(|t| t - known_costs).filter(|r| *r > 0);

    html! {
        @if fin.consolidated { p.note.esef-note { (c.t("fin.esef_note")) } }
        div.charts-grid.mt-4 {
            (chart_card(
                c.t("fin.chart.income"),c.t("fin.chart.income_tip"),
                grouped_bars(c, "ch-income", c.t("fin.chart.income"), &labels, &money_series, Unit::Tkr),
                legend(&[(money_series[0].name.as_str(), "s1"), (money_series[1].name.as_str(), "s2"), (money_series[2].name.as_str(), "s3")]),
                data_table(c, c.t("fin.chart.income"), &labels, &money_series, Unit::Tkr),
            ))
            (chart_card(
                c.t("fin.chart.margins"), c.t("fin.chart.margins_tip"),
                line_chart(c, "ch-margins", c.t("fin.chart.margins"), &labels, &margin_series, Unit::Percent, sector_reference(c, medians, |m| m.margin, Unit::Percent)),
                html! {
                    (legend(&[(margin_series[0].name.as_str(), "s2"), (margin_series[1].name.as_str(), "s1"), (margin_series[2].name.as_str(), "s3")]))
                    @if medians.is_some() { p.note { (c.t("fin.sector_dashed")) } }
                },
                data_table(c, c.t("fin.chart.margins"), &labels, &margin_series, Unit::Percent),
            ))
            (chart_card(
                c.t("fin.chart.solidity"), c.t("tip.solidity"),
                line_chart(c, "ch-solidity", c.t("fin.chart.solidity"), &labels, &solidity_series, Unit::Percent, sector_reference(c, medians, |m| m.solidity, Unit::Percent)),
                html! { @if medians.is_some() { p.note { (c.t("fin.sector_dashed")) } } },
                data_table(c, c.t("fin.chart.solidity"), &labels, &solidity_series, Unit::Percent),
            ))
            (chart_card(
                c.t("fin.chart.liquidity"), c.t("tip.liquidity"),
                line_chart(c, "ch-liquidity", c.t("fin.chart.liquidity"), &labels, &liquidity_series, Unit::Percent, sector_reference(c, medians, |m| m.liquidity, Unit::Percent)),
                html! {
                    (legend(&[(liquidity_series[0].name.as_str(), "s1"), (liquidity_series[1].name.as_str(), "s4")]))
                    @if medians.is_some() { p.note { (c.t("fin.sector_dashed")) } }
                },
                data_table(c, c.t("fin.chart.liquidity"), &labels, &liquidity_series, Unit::Percent),
            ))
            @if structure_ok {
                (chart_card(
                    c.t("fin.chart.structure"), c.t("fin.chart.structure_tip"),
                    stacked_bars(c, "ch-structure", c.t("fin.chart.structure"), &labels, &structure_series, Unit::Tkr),
                    legend(&structure_series.iter().zip(["s1", "s2", "s3", "s5"]).filter(|(s, _)| has(s)).map(|(s, k)| (s.name.as_str(), k)).collect::<Vec<_>>()),
                    data_table(c, c.t("fin.chart.structure"), &labels, &structure_series, Unit::Tkr),
                ))
            }
            @if latest.revenue.is_some_and(|r| r > 0) && known_costs > 0 {
                (cost_card(c, latest, &cost_parts, rest_costs))
            }
        }

        div.card.mt-4 {
            h2.card-title { (c.t("fin.ratios")) (info_btn(c.t("fin.ratios"), c.t("fin.ratios_tip"))) }
            (statement_table(c, c.t("fin.ratios"), &ys, html! {
                (ratio_row(c, term(c.t("fin.operating_margin"), c.t("tip.operating_margin")), &ys, |y| y.operating_margin(), Unit::Percent))
                (ratio_row(c, term(c.t("term.margin"), c.t("tip.margin")), &ys, |y| y.margin(), Unit::Percent))
                (ratio_row(c, term(c.t("fin.net_margin"), c.t("tip.net_margin")), &ys, |y| y.net_margin(), Unit::Percent))
                (ratio_row(c, term(c.t("fin.roe"), c.t("tip.roe")), &ys, |y| y.roe(), Unit::Percent))
                (ratio_row(c, term(c.t("fin.roa"), c.t("tip.roa")), &ys, |y| y.roa(), Unit::Percent))
                (ratio_row(c, term(c.t("fin.asset_turnover"), c.t("tip.asset_turnover")), &ys, |y| y.asset_turnover(), Unit::Times))
                (ratio_row(c, term(c.t("term.solidity"), c.t("tip.solidity")), &ys, |y| y.solidity(), Unit::Percent))
                (ratio_row(c, term(c.t("term.liquidity"), c.t("tip.liquidity")), &ys, |y| y.liquidity(), Unit::Percent))
                (ratio_row(c, term(c.t("fin.current_ratio"), c.t("tip.current_ratio")), &ys, |y| y.current_ratio(), Unit::Percent))
                (ratio_row(c, term(c.t("fin.cash_ratio"), c.t("tip.cash_ratio")), &ys, |y| y.cash_ratio(), Unit::Percent))
                (ratio_row(c, term(c.t("fin.debt_to_equity"), c.t("tip.debt_to_equity")), &ys, |y| y.debt_to_equity(), Unit::Times))
                (ratio_row(c, term(c.t("fin.interest_cover"), c.t("tip.interest_cover")), &ys, |y| y.interest_cover(), Unit::Times))
                (ratio_row(c, term(c.t("fin.personnel_share"), c.t("tip.personnel_share")), &ys, |y| y.personnel_share(), Unit::Percent))
                (money_row(c, c.t("fin.working_capital"), &ys, |y| y.working_capital(), false))
                (money_row(c, c.t("fin.revenue_per_employee"), &ys, |y| y.revenue_per_employee(), false))
                tr {
                    th scope="row" { (c.t("fin.employees")) }
                    @for y in &ys { td.right.num.mono { @match y.employees { Some(e) => { (c.dec(e, if e.fract() == 0.0 { 0 } else { 1 })) } None => { span.muted { "—" } } } } }
                }
            }))
            p.note { (c.t("fin.ratios_note")) }
        }

        div.card.mt-4 {
            h2.card-title { (c.t("fin.income_statement")) }
            (statement_table(c, c.t("fin.income_statement"), &ys, html! {
                (money_row(c, c.t("fin.revenue"), &ys, |y| y.revenue, true))
                (money_row(c, c.t("fin.operating_income"), &ys, |y| y.operating_income, false))
                (money_row(c, c.t("fin.goods_cost"), &ys, |y| y.goods_cost, false))
                (money_row(c, c.t("fin.other_external"), &ys, |y| y.other_external, false))
                (money_row(c, c.t("fin.personnel_cost"), &ys, |y| y.personnel_cost, false))
                (money_row(c, c.t("fin.depreciation"), &ys, |y| y.depreciation, false))
                (money_row(c, c.t("fin.operating_costs"), &ys, |y| y.operating_costs, false))
                (money_row(c, c.t("fin.operating_result"), &ys, |y| y.operating_result, true))
                (money_row(c, c.t("fin.financial_net"), &ys, |y| y.financial_net, false))
                (money_row(c, c.t("fin.result"), &ys, |y| y.result, true))
                (money_row(c, c.t("fin.result_before_tax"), &ys, |y| y.result_before_tax, false))
                (money_row(c, c.t("fin.tax"), &ys, |y| y.tax, false))
                (money_row(c, c.t("fin.result_year"), &ys, |y| y.net_result, true))
            }))
        }

        div.card.mt-4 {
            h2.card-title { (c.t("fin.balance_sheet")) }
            (statement_table(c, c.t("fin.balance_sheet"), &ys, html! {
                (money_row(c, c.t("fin.fixed_assets"), &ys, |y| y.fixed_assets, false))
                (money_row(c, c.t("fin.inventory"), &ys, |y| y.inventory, false))
                (money_row(c, c.t("fin.trade_receivables"), &ys, |y| y.trade_receivables, false))
                (money_row(c, c.t("fin.short_receivables"), &ys, |y| y.short_receivables, false))
                (money_row(c, c.t("fin.cash"), &ys, |y| y.cash, false))
                (money_row(c, c.t("fin.current_assets"), &ys, |y| y.current_assets, false))
                (money_row(c, c.t("fin.assets"), &ys, |y| y.assets, true))
                (money_row(c, c.t("fin.share_capital"), &ys, |y| y.share_capital, false))
                (money_row(c, c.t("fin.restricted_equity"), &ys, |y| y.restricted_equity, false))
                (money_row(c, c.t("fin.free_equity"), &ys, |y| y.free_equity, false))
                (money_row(c, c.t("fin.equity"), &ys, |y| y.equity, true))
                (money_row(c, c.t("fin.untaxed_reserves"), &ys, |y| y.untaxed_reserves, false))
                (money_row(c, c.t("fin.long_term_debt"), &ys, |y| y.long_term_debt, false))
                (money_row(c, c.t("fin.short_term_debt"), &ys, |y| y.short_term_debt, false))
                (money_row(c, c.t("fin.trade_payables"), &ys, |y| y.trade_payables, false))
            }))
        }
        p.note { (c.tf(if fin.consolidated { "bok.source_esef" } else { "bok.source" }, &[latest.period_end.as_str()])) }
    }
}

/// Tarjeta con el reparto de los costes de explotación del último ejercicio sobre la facturación.
fn cost_card(c: &Ctx, latest: &FinancialYear, parts: &[(String, Option<i64>, &'static str)], rest: Option<i64>) -> Markup {
    let revenue = latest.revenue.unwrap_or(1).max(1) as f64;
    let mut segs: Vec<(String, i64, &'static str)> = parts.iter().filter_map(|(n, v, k)| v.filter(|x| *x > 0).map(|x| (n.clone(), x, *k))).collect();
    if let Some(r) = rest {
        segs.push((c.t("fin.other_costs").to_string(), r, "s5"));
    }
    let total: i64 = segs.iter().map(|(_, v, _)| *v).sum();
    let margin = latest.operating_result.map(|r| r as f64 / revenue * 100.0);
    html! {
        div.card {
            h2.card-title { (c.tf("fin.chart.costs", &[latest.label.as_str()])) (info_btn(c.t("fin.chart.costs_short"), c.t("fin.chart.costs_tip"))) }
            div.cost-bar role="img" aria-label=(c.t("fin.chart.costs_short")) {
                @for (name, v, class) in &segs {
                    @let pct = *v as f64 / revenue * 100.0;
                    @let tip = format!("{name}: {} ({} {})", c.money(*v), c.pct1(pct), c.t("fin.of_revenue"));
                    span class={"cost-seg " (class)} style=(format!("flex-grow:{}", v)) data-tip=(tip) tabindex="0" aria-label=(tip) {}
                }
            }
            ul.cost-list {
                @for (name, v, class) in &segs {
                    li { span class={"swatch " (class)} {} span.cost-name { (name) } span.cost-val.num.mono { (c.money(*v)) " · " (c.pct1(*v as f64 / revenue * 100.0)) } }
                }
                @if let Some(m) = margin {
                    li.cost-total { span.cost-name { (icon_sized("check-circle", "icon-sm")) (c.t("fin.operating_result")) } span.cost-val.num.mono { (c.pct1(m)) " " (c.t("fin.of_revenue")) } }
                }
            }
            p.note { (c.tf("fin.costs_total", &[&c.money(total), &c.pct1(total as f64 / revenue * 100.0)])) }
        }
    }
}

// ───────────────────────── Pestaña Personas y memoria ─────────────────────────

pub struct Person {
    pub name: String,
    pub role: String,
}

/// Firmantes del informe más reciente: nombre y cargo, agrupados por el contexto del informe.
pub fn signatories(facts: &[RawFact]) -> Vec<Person> {
    use std::collections::BTreeMap;
    let mut by_ctx: BTreeMap<&str, (String, String, String)> = BTreeMap::new();
    for f in facts {
        let Some(text) = f.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) else { continue };
        let slot = by_ctx.entry(f.ctx.as_str()).or_default();
        match f.local() {
            "UnderskriftHandlingTilltalsnamn" | "UnderskriftFaststallelseintygForetradareTilltalsnamn" => slot.0 = text.to_string(),
            "UnderskriftHandlingEfternamn" | "UnderskriftFaststallelseintygForetradareEfternamn" => slot.1 = text.to_string(),
            "UnderskriftHandlingRoll" | "UnderskriftFaststallelseintygForetradareForetradarroll" => slot.2 = text.to_string(),
            _ => {}
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    by_ctx
        .into_values()
        .filter(|(first, last, _)| !first.is_empty() || !last.is_empty())
        .map(|(first, last, role)| Person { name: format!("{first} {last}").trim().to_string(), role })
        .filter(|p| seen.insert((p.name.clone(), p.role.clone())))
        .collect()
}

/// Primer texto del concepto en la lista de hechos (el del informe más reciente si se pasan en ese orden).
fn first_text<'a>(facts: &'a [RawFact], local: &str) -> Option<&'a str> {
    facts.iter().find(|f| f.local() == local).and_then(|f| f.text.as_deref())
}

const TEXT_SECTIONS: [(&str, &str); 4] = [
    ("AllmantVerksamheten", "ppl.section.activity"),
    ("VasentligaHandelserRakenskapsaret", "ppl.section.events"),
    ("ArsstammaResultatDispositionGodkannaStyrelsensForslag", "ppl.section.dividend"),
    ("RedovisningsVarderingsprinciper", "ppl.section.principles"),
];

pub fn people_tab(c: &Ctx, org: &Organisation, reports: &[(ReportInfo, Vec<RawFact>)]) -> Markup {
    let Some((info, facts)) = reports.first() else {
        return no_accounts_card(c, org);
    };
    let people = signatories(facts);
    let employees = facts.iter().find(|f| f.local() == "MedelantaletAnstallda" && f.value.is_some() && f.dims.is_empty()).and_then(|f| f.value);
    html! {
        div.card.mt-4 {
            h2.card-title { (c.t("ppl.signatories")) (info_btn(c.t("ppl.signatories"), c.t("ppl.signatories_tip"))) }
            @if people.is_empty() {
                p.muted { (c.t("ppl.none")) }
            } @else {
                div.table-wrap.flat {
                    table {
                        caption.sr-only { (c.t("ppl.signatories")) }
                        thead { tr { th scope="col" { (c.t("ppl.name")) } th scope="col" { (c.t("ppl.role")) } } }
                        tbody { @for p in &people { tr { td { strong { (p.name) } } td { (p.role) } } } }
                    }
                }
            }
            @if let Some(e) = employees { p.mt-3 { (c.t("fin.employees")) ": " strong { (c.dec(e, 0)) } } }
            p.note { (c.tf("ppl.source", &[info.period_end.as_str(), org.namn.as_str()])) }
        }
        @for (concept, key) in TEXT_SECTIONS {
            @if let Some(text) = first_text(facts, concept) {
                div.card.mt-4 {
                    h2.card-title { (c.t(key)) }
                    @for para in text.split("\n\n").filter(|p| !p.trim().is_empty()).take(8) { p.report-text { (para.trim()) } }
                }
            }
        }
        p.note { (c.t("ppl.swedish_note")) }
    }
}

// ───────────────────────── Pestaña Datos del informe ─────────────────────────

fn period_label(f: &RawFact) -> String {
    f.instant.clone().unwrap_or_else(|| format!("{} → {}", f.start.clone().unwrap_or_default(), f.end.clone().unwrap_or_default()))
}

/// Todos los datos guardados de la empresa: informes leídos y cada cifra con su concepto y periodo.
pub fn data_tab(c: &Ctx, org: &Organisation, reports: &[(ReportInfo, Vec<RawFact>)]) -> Markup {
    if reports.is_empty() {
        return no_accounts_card(c, org);
    }
    html! {
        div.card.mt-4 {
            h2.card-title { (c.t("dat.reports")) (info_btn(c.t("dat.reports"), c.t("dat.reports_tip"))) }
            div.table-wrap.flat {
                table {
                    caption.sr-only { (c.t("dat.reports")) }
                    thead { tr {
                        th scope="col" { (c.t("dat.period_end")) }
                        th scope="col" { (c.t("dat.registered")) }
                        th.right scope="col" { (c.t("dat.facts")) }
                        th scope="col" { (c.t("dat.document")) }
                    } }
                    tbody {
                        @for (info, _) in reports {
                            tr {
                                td.num { (info.period_end) }
                                td.num { (info.registered) }
                                td.right.num.mono { (c.int(info.fact_count)) }
                                td { span.mono.muted { (info.doc_id) } }
                            }
                        }
                    }
                }
            }
            p.note { (c.t("dat.note")) }
        }
        @for (info, facts) in reports.iter().take(3) {
            @let numeric: Vec<&RawFact> = facts.iter().filter(|f| f.value.is_some()).collect();
            div.card.mt-4 {
                h2.card-title { (c.tf("dat.facts_of", &[info.period_end.as_str()])) }
                details.chart-data {
                    summary { (c.tf("dat.show_numeric", &[&c.int(numeric.len() as i64)])) }
                    div.table-wrap.flat {
                        table.data-table {
                            caption.sr-only { (c.tf("dat.facts_of", &[info.period_end.as_str()])) }
                            thead { tr {
                                th scope="col" { (c.t("dat.concept")) }
                                th scope="col" { (c.t("dat.period")) }
                                th.right scope="col" { (c.t("dat.value")) }
                                th scope="col" { (c.t("dat.breakdown")) }
                            } }
                            tbody {
                                @for f in &numeric {
                                    tr {
                                        td.mono { (f.local()) }
                                        td.num.nowrap { (period_label(f)) }
                                        td.right.num.mono { (c.dec(f.value.unwrap_or(0.0), if f.value.is_some_and(|v| v.fract() != 0.0) { 3 } else { 0 })) " " span.muted { (f.unit.clone().unwrap_or_default()) } }
                                        td.muted { (f.dims) }
                                    }
                                }
                            }
                        }
                    }
                }
                @let texts: Vec<&RawFact> = facts.iter().filter(|f| f.value.is_none() && f.text.is_some()).collect();
                details.chart-data {
                    summary { (c.tf("dat.show_text", &[&c.int(texts.len() as i64)])) }
                    div.table-wrap.flat {
                        table.data-table {
                            caption.sr-only { (c.t("dat.text_facts")) }
                            thead { tr { th scope="col" { (c.t("dat.concept")) } th scope="col" { (c.t("dat.text")) } } }
                            tbody {
                                @for f in &texts { tr { td.mono { (f.local()) } td.report-text { (f.text.clone().unwrap_or_default().chars().take(600).collect::<String>()) } } }
                            }
                        }
                    }
                }
            }
        }
    }
}
