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
        None => tail_nonempty(pane),
    }
}

/// prompt の位置に依らない**直近 [`TAIL_LINES`] 非空行**（prompt 不在の pane の探索域）。
///
/// idle の判定には使わない（[`prompt_region`]）——この域は statusline の高さで埋まるので、
/// 入力欄の上に描かれる走行中の印に届かない（実測 2026-09-11: statusline 3 行 + 区切り 2 行 +
/// prompt 行で 6 行が尽き、spinner 行は下から 8 非空行目に在った）。
pub fn tail_nonempty(pane: &str) -> Vec<&str> {
    let mut tail: Vec<&str> = pane
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(TAIL_LINES)
        .collect();
    tail.reverse();
    tail
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

/// spinner 行の行頭に描かれる frame の字（本体 2.1.268 の配列を実読: unicode 版
/// `· ✢ ✳ ✶ ✻ ✽` と ASCII 版 `· ✢ * ✶ ✻ ✽`）。
///
/// assistant 本文の行頭 `●`・tool の `⏺` / `⎿`・markdown の `-` はこの集合に無い＝本文が
/// `… (3 件)` の形を持っても spinner と読まない（lens-94 HIGH-3 の絞り・planner 裁定 2026-09-11）。
pub const SPINNER_GLYPHS: [char; 7] = ['·', '✢', '✳', '✶', '✻', '✽', '*'];

/// 入力欄より**上**で走行中を名乗る字（spinner 行が banner に置き換わる周・compaction 中）。
///
/// API 待ちの banner は 4 形あり（本体の実装を実読・lens-94 HIGH-2）、`Retrying in` /
/// `will retry in` / `next try in` / `waiting up to` で 4 形とも 1 つ以上に当たる。compaction 中は
/// `…` すら描かれない専用の行（lens-94 MEDIUM-4）。字面での判定は暫定で、typed 化は `s2-07l.95`。
pub const BUSY_MARKS_ABOVE: [&str; 5] = [
    "Retrying in",
    "will retry in",
    "next try in",
    "waiting up to",
    "Compacting conversation",
];

/// 入力欄より**下**（statusline）で走行中を名乗る字。
///
/// 旧い版の印。現行の版では API 再試行行にしか描かれない（本体の文字列を実読 2026-09-11）ので
/// **これだけでは走行中を読めない**——2026-09-11 に `/rebrief` 走行中の席へ `/clear` が送られた
/// （bd `s2-07l.94`）。上の域では見ない: 本文がこの字を**引用**した idle な席を永久に busy と読む
/// （lens-94 HIGH-3・席は idle だと出力を出さないので pane が変わらず脱出できない）。
pub const BUSY_MARKS_BELOW: [&str; 1] = ["esc to interrupt"];

/// 入力欄より**上**で走行中の印を探す非空行の数。
///
/// 実測（2026-09-11・3 席）では spinner 行は prompt 行の 2〜3 非空行上に在る（区切り線 1 行と、
/// Tip・auto-update の注意・queue された入力の写しが 0〜2 行）。2 倍の余裕を持たせつつ、それ以上
/// 遡らないのは、前の turn の出力に spinner の形の行が写っている（tool の結果に pane の写しが
/// 載る等）周に idle な席を busy と読み続けないためである。行は `capture-pane -p` の**折返し後**
/// の物理行なので、狭い pane では Tip 行が 2 行に折れて余裕が 1 行減る（lens-94 MEDIUM-6）。
const ABOVE_LINES: usize = 6;

/// 走行中の印を探す域: **最後の prompt 行より上の非空 [`ABOVE_LINES`] 行 + 下の全行**。
///
/// 上を数で切り下を全部取るのは、印の位置が 2 通りあるためである——spinner は入力欄の上
/// （版によって statusline にも）に描かれ、statusline の高さは設定で変わる（実測 2026-09-11:
/// 3 行）。末尾から固定行数を取る形（[`tail_nonempty`]）は statusline が高いほど上に届かず、
/// **走行中の席を idle と読む**（bd `s2-07l.94`）。prompt 行が無い pane は空（呼び側は
/// [`input_tail`] が `None` で先に fail-closed へ倒れる）。
pub fn prompt_region(pane: &str) -> Vec<&str> {
    let Some((mut above, below)) = split_regions(pane) else {
        return Vec::new();
    };
    above.extend(below);
    above
}

/// [`prompt_region`] を上（非空 [`ABOVE_LINES`] 行）と下（全行）に分けて返す。prompt 行が無ければ `None`。
fn split_regions(pane: &str) -> Option<(Vec<&str>, Vec<&str>)> {
    let lines: Vec<&str> = pane.lines().collect();
    let at = lines.iter().rposition(|line| line.contains(PROMPT))?;
    let mut above: Vec<&str> = lines
        .iter()
        .take(at)
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(ABOVE_LINES)
        .copied()
        .collect();
    above.reverse();
    let below: Vec<&str> = lines
        .iter()
        .skip(at.saturating_add(1))
        .filter(|line| !line.trim().is_empty())
        .copied()
        .collect();
    Some((above, below))
}

/// spinner 行の形か: `<frame の 1 字> <語または文>…` に、括弧が在るなら `(<経過時間>` が続く
/// （例 `✻ Sublimating… (19m 36s · ↓ 36.4k tokens)`・`✻ Reviewing the seat idle predicate… (2m 3s)`・
/// `✻ Sublimating…`〔開始 16 秒未満は括弧が無い〕・旧版の `✻ Thinking… (23s · esc to interrupt)`）。
///
/// 語は版と周で変わり、todo が走る周は todo の文（空白入り）になる（lens-94 HIGH-1）ので、
/// **語ではなく形**で読む: 行頭は [`SPINNER_GLYPHS`] の 1 字、`…` で切れ、括弧の中は経過時間
/// （[`starts_with_elapsed`]）。turn の完了行（`✻ Crunched for 10m 28s · done`）は `…` を持たず、
/// 本文の `● 直した… (3 件)` は行頭も括弧の中も外れる。ASCII 版の `*` は markdown の箇条書きと
/// 同じ字なので、括弧無しの形では取らない。
pub fn is_spinner_line(line: &str) -> bool {
    let mut chars = line.trim_start().chars();
    let Some(glyph) = chars.next() else {
        return false;
    };
    if !SPINNER_GLYPHS.contains(&glyph) {
        return false;
    }
    let Some(rest) = chars.as_str().strip_prefix(' ') else {
        return false;
    };
    match rest.split_once("… (") {
        Some((text, tail)) => !text.is_empty() && starts_with_elapsed(tail),
        None => glyph != '*' && rest.len() > "…".len() && rest.ends_with('…'),
    }
}

/// 括弧の中が経過時間で始まるか: `<n>h` / `<n>m` / `<n>s` を空白で 1〜3 つ並べ、直後が `)` か ` ·`。
fn starts_with_elapsed(tail: &str) -> bool {
    let mut rest = tail;
    for _ in 0..3 {
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        let Some(after_digits) = rest.get(digits..) else {
            return false;
        };
        if digits == 0 {
            return false;
        }
        let mut units = after_digits.chars();
        if !matches!(units.next(), Some('h' | 'm' | 's')) {
            return false;
        }
        let after_unit = units.as_str();
        if after_unit.starts_with(')') || after_unit.starts_with(" ·") {
            return true;
        }
        let Some(next) = after_unit.strip_prefix(' ') else {
            return false;
        };
        rest = next;
    }
    false
}

/// 入力欄より上の行が走行中の印を持つか（[`BUSY_MARKS_ABOVE`] のどれかを含む ∨ spinner の形）。
fn is_busy_above(line: &str) -> bool {
    BUSY_MARKS_ABOVE.iter().any(|mark| line.contains(mark)) || is_spinner_line(line)
}

/// 入力欄より下の行が走行中の印を持つか（[`BUSY_MARKS_BELOW`]）。
fn is_busy_below(line: &str) -> bool {
    BUSY_MARKS_BELOW.iter().any(|mark| line.contains(mark))
}

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

/// 席が idle か（裁定 (e)）: 入力欄が空 ∧ 上の域（[`ABOVE_LINES`] 非空行）に spinner の形も
/// [`BUSY_MARKS_ABOVE`] も無い ∧ 下の全行に [`BUSY_MARKS_BELOW`] が無い。
///
/// prompt 行を特定できない pane は idle と名乗らない（[`input_tail`] が `None`）＝
/// fail-closed。読めない席へ注入しない側へ倒すためである。域と印の理由はそれぞれの doc に
/// 書いた（印は入力欄の上に出る・語は版で変わる・本文の引用を上の域で印に数えない）。
pub fn is_idle(pane: &str) -> bool {
    let Some((above, below)) = split_regions(pane) else {
        return false;
    };
    input_tail(pane).is_some_and(str::is_empty)
        && !above.iter().any(|line| is_busy_above(line))
        && !below.iter().any(|line| is_busy_below(line))
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
