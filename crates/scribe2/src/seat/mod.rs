//! `seat` subcommand の面（設計 docs/design/seat-autonomy.md §3・seat-state.md）: 開発 session の
//! context 使用量を読む [`meter`]、tmux pane へ 1 行を送る [`inject`]、席の状態を hook の打刻で
//! typed に持つ [`state`]。
//!
//! **env も HOME も読まない**（憲法 C2.2）。pane は `--target`（または明示された
//! `--capture-file`）だけを見て、transcript は `--transcript` で明示された file だけを
//! 読む。tmux は外部 process の呼出しで、crate 依存は増えない（憲法 A3 非該当）。
//! **pane の字面は席の busy / idle の判定入力にしない**（憲法 C3.3・ADR-0015）: 字面を読むのは
//! statusline の数値（[`meter`]）と注入の送達確認（[`inject`]）だけである。
//!
//! 出力は行を組んで返すだけで、stdout / stderr へは bin 側の `emit` / `emit_err` が
//! 書く（憲法 C2）。

pub mod cli;
pub mod cycle;
pub mod externalize;
pub mod heartbeat;
pub mod inject;
pub mod meter;
pub mod state;
pub mod tick;
pub mod wm;

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

/// pane id（`%N`・生成 hooks.json の shell 行が `$TMUX_PANE` から渡す）から、tick と同じ形の
/// target（`session:window`）を解く（設計 seat-state.md §3・ADR-0015 §2.2）。撃てない・どちらかの
/// 名が空は `None`（打刻しない側）。
///
/// **両方の名が非空のときだけ** target とする: 無い pane id を渡された `display-message` は版に
/// よって rc 非 0 でなく **`:`（両方空）を rc 0 で返す**（実測 2026-09-11・tmux 3.6b）。空を通すと
/// 潰した dir 名 `_` の席が生まれ、存在しない席へ打刻を積む。
pub fn target_of_pane(socket: Option<&str>, pane: &str) -> Option<String> {
    let out = tmux_stdout(
        socket,
        &["display-message", "-p", "-t", pane, "#{session_name}:#{window_name}"],
    )?;
    let target = out.trim();
    let (session, window) = target.split_once(':')?;
    (!session.is_empty() && !window.is_empty()).then(|| target.to_owned())
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

/// prompt の位置に依らない**直近 [`TAIL_LINES`] 非空行**（prompt 不在の pane の statusline 探索域）。
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

/// 席の置き場（`<state_dir>/seat/<潰した target>/`）。
///
/// 便 2 の記録 file の**親**から導く＝dir 名の字面を 2 面に持たない。写して持つと片方だけ
/// 変わったときに heartbeat と記録が別の dir へ散り、鮮度が永久に stale になる。
pub fn seat_dir(state_dir: &Path, target: &str) -> PathBuf {
    let path = inject::tick_path(state_dir, target);
    path.parent().map_or_else(|| path.clone(), Path::to_path_buf)
}

/// 置き場の解決の出所（語彙 Provenance・憲法 C10）。**2 値で閉じる**（解決順序 `--state-dir` >
/// git 設定の 2 経路しか無く、第 3 の経路を足すときは variant を足す＝行の `source=` が経路の
/// 全数を名乗る・憲法 C2）。
#[derive(Clone, Copy)]
pub enum Provenance {
    /// `--state-dir` で渡された。
    Flag,
    /// git の設定解決（`git config --get <NAME>.stateDir`）から読んだ。**repo-local に限らない**
    /// （global や git 自身の env 経由の設定も同じ 1 語で名乗る＝器は git の解決を分解しない）。
    GitConfig,
}

impl Provenance {
    /// 行と記録に使う字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Flag => "flag",
            Self::GitConfig => "git-config",
        }
    }
}

/// 解決した置き場（**出所付き**・憲法 C10）。
///
/// 成功行が「書いた」だけでなく「どこへ・何から解いて」を名乗るための型である。
/// rc 0 の成功行が別 dir へ書いていた事故（2026-09-10・repo-local の git 設定が死んだ
/// probe dir を指したまま残っていた）を、行の側で見える形にする（`s2-07l.70`）。
pub struct StateDir {
    /// 解決した path（**絶対**にして持つ: 相対の flag は cwd に依存し「どこへ」を名乗れない）。
    pub path: PathBuf,
    /// 解決の出所。
    pub source: Provenance,
}

impl StateDir {
    /// 成功行と記録の末尾に足す字面（既存 token の後ろ＝名前・順序・書式を変えない）。
    ///
    /// **path は行末**に置く: path は行で唯一潰さない外部の字面で、空白や ` source=` を含みうる。
    /// 出所を先に出せば、読み手は「` state_dir=` 以降の全部が path」と一意に読める。
    pub fn suffix(&self) -> String {
        format!(
            " source={} state_dir={}",
            self.source.as_str(),
            self.path.display()
        )
    }

    /// この置き場の host の受付札の置き場（[`host_slots_dir`]）。
    pub fn slots_dir(&self) -> PathBuf {
        host_slots_dir(&self.path)
    }
}

/// host 単位の受付札の置き場（`<state_dir の親>/<NAME>-host/slots/`・設計 gate-cost.md §3.2）。
///
/// **state dir の親から導く**——同じ host の state dir は 1 つの親（host の state root）に置く
/// 運用なので、project をまたいで 1 つの dir になる。env（`XDG_RUNTIME_DIR` / `HOME` /
/// `TMPDIR`）は読まない（憲法 C2.2）。親を持たない path（`/`）はそれ自身を親と読む。
pub fn host_slots_dir(state_dir: &Path) -> PathBuf {
    state_dir
        .parent()
        .unwrap_or(state_dir)
        .join(format!("{}-host", crate::name::NAME))
        .join("slots")
}

/// 置き場を解く。`--state-dir` が上書きし、無ければ repo の git 設定から読む。
///
/// `current_dir` は syscall であって env ではない（憲法 C2.2・hook 側と同じ扱い）。
/// 絶対化は `std::path::absolute`（symlink も存在も見ない＝書く先そのものの名前）。
pub fn state_dir_of(state_dir: Option<&str>) -> Option<StateDir> {
    let (path, source) = match state_dir {
        Some(found) => (PathBuf::from(found), Provenance::Flag),
        None => {
            let cwd = std::env::current_dir().ok()?;
            let root = crate::hook::vessel::repo_root(&cwd)?;
            (crate::hook::vessel::state_dir(&root)?, Provenance::GitConfig)
        }
    };
    let path = std::path::absolute(path).ok()?;
    Some(StateDir { path, source })
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

/// 退避物の名前の前置き（FR23・[`externalize`] と共有）。
pub const WM_PREFIX: &str = "working-memory.";
/// 退避物の名前の後置き。
pub const WM_SUFFIX: &str = ".md";
/// consume 済みの後置き（**mv が consume の実体**ゆえ、この字で終わらない `.md` が未 consumed）。
pub const WM_CONSUMED: &str = ".consumed.md";
/// frontmatter の区切り。
pub const FRONTMATTER: &str = "---";
/// frontmatter を読む上限（byte）。**全文を読まない**（退避物は数十 KB になる）。
const FRONTMATTER_CAP: u64 = 8192;
/// 席を名乗る frontmatter の key。
pub const SEAT_KEY: &str = "seat:";

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

/// 退避物の frontmatter が名乗る席。名乗りが無い・読めないなら `None`（[`externalize`] の carry 元の
/// 弁別もこの 1 本を通る＝自席の数え方を 2 面に持たない）。
pub fn seat_of(path: &Path) -> Option<String> {
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
