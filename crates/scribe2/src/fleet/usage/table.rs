//! 人の読む表の群（設計 docs/design/fleet-usage.md §14・契約表の行 d・`s2-07l.544`）。
//!
//! replay から口座 1 つの行を組み（[`table_row`]）、見出しと列幅を揃えて表にする（[`table`]）群である。
//! `fleet/usage.rs` からの**純移動**で、歯は 1 本も足していない（親に残る in-file の歯と e2e が従来どおり測る）。
//! 外の呼び手は 0 で、親の本体と歯からだけ入る。上げた可視性は `table_row` の `pub(super)` 1 語だけである。

// flip-check: moved s2-07l.544

use super::{latest_rows, RESETS_NONE};
use crate::fleet::{Allowance, WindowKind};
use crate::seat::StateDir;

/// 表の値の無い欄（未計測の口座・登録の無い口座・model 窓なし）。
const CELL_NONE: &str = "-";

/// 表の列の区切り。
const CELL_GAP: &str = "  ";

/// 表の見出し（2 行目・列の順は固定）。
const TABLE_HEAD: [&str; 6] = ["account", "5h", "7d", "model", "seat", "resets"];

/// 表の 1 口座分（[`table`] の入力・値は字面で持つ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRow {
    /// 口座の label。
    pub account: String,
    /// 5 時間窓（`13%` / `unmeasured:<reason>` / `-`）。
    pub five_hour: String,
    /// 7 日窓（同上）。
    pub seven_day: String,
    /// model 窓（`Fable:75%` を `,` で並べる・無ければ `-`）。
    pub model: String,
    /// 登録 row が持つ口座ならその役割の名・無ければ `-`。
    pub seat: String,
    /// 5 時間窓の reset 時刻（無ければ `-`）。
    pub resets: String,
}

impl TableRow {
    /// 列の順の値。
    fn cells(&self) -> [&str; 6] {
        [&self.account, &self.five_hour, &self.seven_day, &self.model, &self.seat, &self.resets]
    }
}

/// 人が読む表（pure・設計 §11 (2)）: 1 行目 = [`StateDir::suffix`] の字面から先頭の空白を落としたもの（出所が先・path が
/// 行末＝第 2 の書式を書かない）、2 行目 = 見出し、以下は口座ごとに 1 行。列幅は見出しと値の最大幅（文字数）で揃える
/// （数を code に書かない）。末尾の列は詰めない（行末に空白を残さない）。
pub fn table(place: &StateDir, rows: &[TableRow]) -> Vec<String> {
    let mut widths: Vec<usize> = TABLE_HEAD.iter().map(|head| head.chars().count()).collect();
    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row.cells()) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let mut lines = vec![place.suffix().trim_start().to_owned()];
    lines.push(aligned(&TABLE_HEAD, &widths));
    lines.extend(rows.iter().map(|row| aligned(&row.cells(), &widths)));
    lines
}

/// 列を幅に揃えて並べる（最後の列は詰めない）。
fn aligned(cells: &[&str], widths: &[usize]) -> String {
    let last = cells.len().saturating_sub(1);
    cells
        .iter()
        .zip(widths)
        .enumerate()
        .map(|(at, (cell, width))| {
            if at == last {
                (*cell).to_owned()
            } else {
                let pad = width.saturating_sub(cell.chars().count());
                format!("{cell}{}", " ".repeat(pad))
            }
        })
        .collect::<Vec<String>>()
        .join(CELL_GAP)
}

/// 口座 1 つの表の行を replay から組む。窓の値は [`latest_rows`]（1 行形と同じ最新の回）・seat 列は登録 row
/// （`State::registrations`・鍵 = 役割 × anchor・同じ口座を持つ row の役割名を重複なく `,` で並べる）。
/// 口座単位の Unmeasured は 5h / 7d の両欄に `unmeasured:<reason>`。
pub(super) fn table_row(label: &str, state: &super::State) -> TableRow {
    let rows = latest_rows(label, &state.allowance).unwrap_or_default();
    let account_level = rows.iter().find_map(|row| match row {
        Allowance::Unmeasured(found) if found.window.is_none() => Some(found.reason),
        Allowance::Measured(_) | Allowance::Unmeasured(_) => None,
    });
    let window_cell = |window: WindowKind| match account_level {
        Some(reason) => format!("unmeasured:{}", reason.as_str()),
        None => rows
            .iter()
            .find(|row| row.key().window == Some(window))
            .map_or_else(|| CELL_NONE.to_owned(), pct_cell),
    };
    let models: Vec<String> = rows
        .iter()
        .filter(|row| row.key().window == Some(WindowKind::SevenDayModel))
        .map(model_cell)
        .collect();
    let resets = rows
        .iter()
        .find_map(|row| match row {
            Allowance::Measured(found) if found.window == WindowKind::FiveHour => {
                Some(found.resets_at.clone().unwrap_or_else(|| RESETS_NONE.to_owned()))
            }
            Allowance::Measured(_) | Allowance::Unmeasured(_) => None,
        })
        .unwrap_or_else(|| CELL_NONE.to_owned());
    let mut roles: Vec<&'static str> = state
        .registrations
        .values()
        .filter(|latest| latest.registration.account == label)
        .map(|latest| latest.registration.role.as_str())
        .collect();
    roles.sort_unstable();
    roles.dedup();
    TableRow {
        account: label.to_owned(),
        five_hour: window_cell(WindowKind::FiveHour),
        seven_day: window_cell(WindowKind::SevenDay),
        model: if models.is_empty() { CELL_NONE.to_owned() } else { models.join(",") },
        seat: if roles.is_empty() { CELL_NONE.to_owned() } else { roles.join(",") },
        resets,
    }
}

/// 窓の欄（`13%` / `unmeasured:<reason>`）。
fn pct_cell(row: &Allowance) -> String {
    match row {
        Allowance::Measured(found) => format!("{}%", found.used_pct),
        Allowance::Unmeasured(found) => format!("unmeasured:{}", found.reason.as_str()),
    }
}

/// model 窓の欄（`Fable:75%` / `Fable:unmeasured:<reason>`・名の無い要素は `unmeasured:<reason>`）。
fn model_cell(row: &Allowance) -> String {
    let name = match row {
        Allowance::Measured(found) => found.model.as_deref(),
        Allowance::Unmeasured(found) => found.model.as_deref(),
    };
    match name {
        Some(name) => format!("{name}:{}", pct_cell(row)),
        None => pct_cell(row),
    }
}
