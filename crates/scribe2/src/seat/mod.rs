//! `seat` subcommand の面（設計 docs/design/seat-autonomy.md §3）: 開発 session の
//! context 使用量を読む [`meter`] と、tmux pane へ 1 行を送る [`inject`]。
//!
//! **env も HOME も読まない**（憲法 C2.2）。pane は `--target`（または明示された
//! `--capture-file`）だけを見て、transcript は `--transcript` で明示された file だけを
//! 読む。tmux は外部 process の呼出しで、crate 依存は増えない（憲法 A3 非該当）。
//!
//! 出力は行を組んで返すだけで、stdout / stderr へは bin 側の `emit` / `emit_err` が
//! 書く（憲法 C2）。

pub mod cli;
pub mod cycle;
pub mod heartbeat;
pub mod inject;
pub mod meter;
pub mod tick;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 開発 session の入力欄を指す prompt の字。
///
/// pane の読みはこの字を **anchor** に使う（statusline は最後の prompt 行より下に
/// 描かれ、打ちかけの text はその右に在る）。
pub const PROMPT: char = '❯';

/// tmux を 1 回撃って stdout を得る。起動失敗・rc 非 0 はいずれも `None`。
fn tmux_stdout(socket: Option<&str>, args: &[&str]) -> Option<String> {
    let mut command = Command::new("tmux");
    if let Some(path) = socket {
        command.arg("-S").arg(path);
    }
    let out = command.args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// tmux を 1 回撃ち、成功したかだけを見る（出力を持たない send 系に使う）。
pub fn tmux_ok(socket: Option<&str>, args: &[&str]) -> bool {
    let mut command = Command::new("tmux");
    if let Some(path) = socket {
        command.arg("-S").arg(path);
    }
    command
        .args(args)
        .output()
        .is_ok_and(|out| out.status.success())
}

/// pane 本文を capture する。撃てなければ `None`（**空文字と区別する**）。
pub fn capture(socket: Option<&str>, target: &str) -> Option<String> {
    tmux_stdout(socket, &["capture-pane", "-p", "-t", target])
}

/// 最後の prompt 行の右（入力欄）の字面。prompt 行が無ければ `None`。
///
/// `None` は「入力欄が空」ではなく **特定できない**である（呼び側は fail-closed に
/// 倒す＝送らない）。
pub fn input_tail(pane: &str) -> Option<&str> {
    pane.lines()
        .rfind(|line| line.contains(PROMPT))?
        .split_once(PROMPT)
        .map(|(_, right)| right.trim())
}

/// statusline を探す域: 最後の prompt 行より下の非空行（prompt 不在なら末尾 6 非空行）。
pub fn search_region(pane: &str) -> Vec<&str> {
    let lines: Vec<&str> = pane.lines().collect();
    match lines.iter().rposition(|line| line.contains(PROMPT)) {
        Some(at) => lines
            .iter()
            .skip(at.saturating_add(1))
            .filter(|line| !line.trim().is_empty())
            .copied()
            .collect(),
        None => {
            let mut tail: Vec<&str> = lines
                .iter()
                .rev()
                .filter(|line| !line.trim().is_empty())
                .take(TAIL_LINES)
                .copied()
                .collect();
            tail.reverse();
            tail
        }
    }
}

/// prompt が 1 行も無い pane で末尾から見る行数。
const TAIL_LINES: usize = 6;

/// target を file 名に使える字面へ潰す（`[A-Za-z0-9_.-]` 以外は `_`）。
///
/// **潰した結果が `.` か `..` になった周は全部 `_` にする**。この 2 形は path の
/// component として上へ抜けるので、`<state_dir>/seat/<target>/tick.jsonl` が
/// `<state_dir>/tick.jsonl` になり **state dir の外を書く**（多段の `../x` は `.._x` に
/// 潰れるので抜けない）。長さは変えない（何文字を潰したかを残す）。
pub fn sanitize_target(target: &str) -> String {
    let squashed: String = target
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if squashed == "." || squashed == ".." {
        return "_".repeat(squashed.len());
    }
    squashed
}

/// 席が「いま動いている」ことを名乗る statusline の字。
///
/// idle の判定はこの字の**不在**で行う（裁定 (e)）。在ることを見るのでなく無いことを見る
/// のは、走っている席へ注入すると打ちかけと混ざるためで、読めない周は idle と名乗らない。
pub const BUSY_MARK: &str = "esc to interrupt";

/// 席の置き場（`<state_dir>/seat/<潰した target>/`）。
///
/// 便 2 の記録 file の**親**から導く＝dir 名の字面を 2 面に持たない。写して持つと片方だけ
/// 変わったときに heartbeat と記録が別の dir へ散り、鮮度が永久に stale になる。
pub fn seat_dir(state_dir: &Path, target: &str) -> PathBuf {
    let path = inject::tick_path(state_dir, target);
    path.parent().map_or_else(|| path.clone(), Path::to_path_buf)
}

/// 置き場を解く。`--state-dir` が上書きし、無ければ repo の git 設定から読む。
///
/// `current_dir` は syscall であって env ではない（憲法 C2.2・hook 側と同じ扱い）。
pub fn state_dir_of(state_dir: Option<&str>) -> Option<PathBuf> {
    match state_dir {
        Some(found) => Some(PathBuf::from(found)),
        None => {
            let cwd = std::env::current_dir().ok()?;
            let root = crate::hook::vessel::repo_root(&cwd)?;
            crate::hook::vessel::state_dir(&root)
        }
    }
}

/// pane 本文を得る。`capture_file` が在れば tmux を **1 度も呼ばない**。
///
/// 明示された file は tmux の**代わり**であって候補ではない: 読めない周に tmux へ落ちると、
/// 「tmux を呼ばない」ための口が live な server を撃つ経路に化ける（meter と同じ扱い）。
pub fn pane_of(socket: Option<&str>, target: &str, capture_file: Option<&str>) -> Option<String> {
    match capture_file {
        Some(path) => std::fs::read_to_string(path).ok(),
        None => capture(socket, target),
    }
}

/// 席が idle か（裁定 (e)）: 入力欄が空 ∧ prompt より下に [`BUSY_MARK`] が無い。
///
/// prompt 行を特定できない pane は idle と名乗らない（[`input_tail`] が `None`）＝
/// fail-closed。読めない席へ注入しない側へ倒すためである。
pub fn is_idle(pane: &str) -> bool {
    input_tail(pane).is_some_and(str::is_empty)
        && !search_region(pane)
            .iter()
            .any(|line| line.contains(BUSY_MARK))
}

/// 退避物の名前の前置き（FR23）。
const WM_PREFIX: &str = "working-memory.";
/// 退避物の名前の後置き。
const WM_SUFFIX: &str = ".md";
/// consume 済みの後置き（**mv が consume の実体**ゆえ、この字で終わらない `.md` が未 consumed）。
const WM_CONSUMED: &str = ".consumed.md";
/// frontmatter の区切り。
const FRONTMATTER: &str = "---";
/// frontmatter を読む上限（byte）。**全文を読まない**（退避物は数十 KB になる）。
const FRONTMATTER_CAP: u64 = 8192;
/// 席を名乗る frontmatter の key。
const SEAT_KEY: &str = "seat:";

/// 自席の未 consumed 退避物の数え（憲法 C11: 「0 件」と「読めない」を混ぜない）。
pub enum WmScan {
    /// 自席の未 consumed が在る（件数）。
    Unconsumed(usize),
    /// **確認した上で** 0 件。
    None,
    /// dir を読めない＝0 件に潰さない。
    Unreadable,
}

/// `<wm-dir>` の未 consumed 退避物のうち、**frontmatter の `seat:` が `target` と一致**
/// するものを数える（裁定 (c)）。
///
/// file 名の sid で弁別しないのは、席の外から回る tick が sid を知らないためである。
/// 名乗りを持たない退避物は自席のものと数えない——他席の退避物を自席の根拠にすると、
/// **別の席の文脈で `/clear` を撃つ**ことになる。
pub fn scan_wm(dir: &Path, target: &str) -> WmScan {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return WmScan::Unreadable;
    };
    let mut found = 0_usize;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_unconsumed_name(&name) {
            continue;
        }
        if seat_of(&entry.path()).is_some_and(|seat| seat == target) {
            found = found.saturating_add(1);
        }
    }
    if found == 0 {
        WmScan::None
    } else {
        WmScan::Unconsumed(found)
    }
}

/// 未 consumed の退避物の名前か（`working-memory.*.md` かつ `.consumed.md` で終わらない）。
fn is_unconsumed_name(name: &str) -> bool {
    name.len() >= WM_PREFIX.len().saturating_add(WM_SUFFIX.len())
        && name.starts_with(WM_PREFIX)
        && name.ends_with(WM_SUFFIX)
        && !name.ends_with(WM_CONSUMED)
}

/// 退避物の frontmatter が名乗る席。名乗りが無い・読めないなら `None`。
fn seat_of(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut head = Vec::new();
    Read::read_to_end(&mut file.take(FRONTMATTER_CAP), &mut head).ok()?;
    let text = String::from_utf8_lossy(&head).into_owned();
    let mut lines = text.lines();
    // 先頭が区切りでない file は frontmatter を持たない（本文の `seat:` を拾わない）。
    if lines.next()?.trim() != FRONTMATTER {
        return None;
    }
    lines
        .take_while(|line| line.trim() != FRONTMATTER)
        .find_map(|line| line.trim().strip_prefix(SEAT_KEY))
        .map(|value| value.trim().trim_matches('"').to_owned())
}

/// 発効している rules 行の整数値。不発効・別の形・不在は `None`（＝判定しない側へ倒す）。
///
/// 閾値の**値は code に焼かない**（憲法 C5・C1「規則はデータ」）。tick と cycle が同じ
/// 読み方をするので、読みはここ 1 箇所に置く。
pub fn int_rule(id: &str) -> Option<u64> {
    let manifest = crate::rules::manifest::Manifest::embedded().ok()?;
    let row = manifest.get(id)?;
    match (row.enabled, &row.value) {
        (true, crate::rules::RuleValue::Int(found)) => Some(*found),
        _ => None,
    }
}
