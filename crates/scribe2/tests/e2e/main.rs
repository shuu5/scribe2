//! 統合 test の唯一の target（憲法 R-C13-2「統合 test file 3 以下」は cargo の
//! integration test **target** の数で数える。以後の leg は module で足す）。
//!
//! doctor の導入先の行（consumer-sync.md §4・AC31・接頭辞 `doctor_consumer_`・`s2-07l.303`）の歯はこの file が持つ
//! （登録は core の `register` で積み、tmux を立てない）。

mod fleet;
mod headless;
mod hook;
mod ledger;
mod ledger_form;
mod ledger_memo;
mod notify;
mod pipe;
mod polarity;
mod prop;
mod rules;
mod seat;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use vessel::fleet::Registration;
use vessel::hook::vessel::digest::{self, PluginRecord};
use vessel::name::{NAME, PLUGIN_DIR};
use vessel::seat::role::Role;

/// 同一 process 内での dir 名衝突を避ける連番。
static SEQ: AtomicU32 = AtomicU32::new(0);

/// repo の外に一意な tmp dir を作る。
///
/// `tempfile` は直接依存の追加（憲法 A3）に当たるので足さない。xtask の
/// `make_tmp_dir` と同形の std だけの helper である。返すのは [`TmpDir`]（drop で dir を再帰削除する包み・
/// 設計 docs/design/gate-cost.md §17・行 h・`s2-07l.343`）で、panic した歯も dir を残さない。
pub fn make_tmp_dir() -> Option<TmpDir> {
    let base = std::env::temp_dir();
    for _ in 0..8 {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = base.join(format!("e2e-{}-{nanos}-{seq}", std::process::id()));
        if std::fs::create_dir(&dir).is_ok() {
            return Some(TmpDir { path: Some(dir) });
        }
    }
    None
}

/// 歯の fixture の一時 dir の包み（`Drop` で再帰削除・panic の unwind の途中でも消える）。
///
/// path として読める（`Deref` で `Path` を貸す）ので、path を繋ぐだけの呼び手は素の `PathBuf` と同じに書ける。
/// 落ちた歯の dir を調べたい周は [`TmpDir::keep`] で path を取り出して guard を降ろす（env は読まない・C2.2）。
#[derive(Debug)]
pub struct TmpDir {
    /// 消す dir（`None` = guard を降ろした後・`keep` の中だけが `None` にする）。
    path: Option<PathBuf>,
}

impl TmpDir {
    /// 包みの path（`Deref` の実体・`PathBuf::as_path` と同じ名＝呼び手の字面を変えない）。
    pub fn as_path(&self) -> &Path {
        self.path.as_deref().unwrap_or(Path::new(""))
    }

    /// symlink を解いた path の包みに替える（同じ dir を指す＝消す対象は変わらない・解けなければ `None` で dir は消える）。
    pub fn canonical(mut self) -> Option<Self> {
        let real = self.as_path().canonicalize().ok()?;
        self.path = Some(real);
        Some(self)
    }

    /// path を取り出して guard を降ろす（降ろした周は drop しても dir が残る）。
    pub fn keep(mut self) -> PathBuf {
        self.path.take().unwrap_or_default()
    }

    /// path を取り出し、guard は**いま走っている歯の thread** に預ける（thread の終端＝歯の終わりで drop・panic でも消える）。
    ///
    /// 素の `PathBuf` を返す局所 helper（write-set の外の呼び手が `tmp().join(..)` の形で一時値を捨てる）が使う口である。
    /// 呼び手の文の終わりで包みが落ちると dir が消えてしまうので、寿命を歯 1 本の thread へ延ばす。
    pub fn held(self) -> PathBuf {
        let path = self.as_path().to_path_buf();
        HELD.with(|held| held.borrow_mut().push(self));
        path
    }
}

thread_local! {
    /// [`TmpDir::held`] が預かった包み（thread の終端で drop される＝libtest は歯 1 本を 1 thread で走らせる）。
    static HELD: std::cell::RefCell<Vec<TmpDir>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            fs::remove_dir_all(path).ok();
        }
    }
}

impl std::ops::Deref for TmpDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        self.as_path()
    }
}

impl AsRef<Path> for TmpDir {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl AsRef<std::ffi::OsStr> for TmpDir {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.as_path().as_os_str()
    }
}

// ─────────── 一時 dir の包みの歯（設計 docs/design/gate-cost.md §17・行 h・`s2-07l.343`） ───────────

/// (a) 包みを drop した後に dir が無い（中に file と sub dir を置いた周も再帰で消える）。
#[test]
fn e2e_fixture_tmp_dir_is_removed_on_drop() {
    let dir = make_tmp_dir().unwrap_or_else(|| panic!("tmp dir を作れる"));
    let path = dir.to_path_buf();
    fs::create_dir_all(dir.join("sub")).unwrap_or_else(|e| panic!("sub dir を作れる: {e}"));
    fs::write(dir.join("sub").join("file"), "x\n").unwrap_or_else(|e| panic!("file を置ける: {e}"));
    assert!(path.join("sub").join("file").is_file(), "前提: drop の前は在る");
    drop(dir);
    assert!(!path.exists(), "drop の後に dir が無い: {}", path.display());
}

/// (b) panic した歯でも dir が消える（unwind を捕まえる口の中で作って落とし、外で不在を測る）。預けた包みも
/// thread の終端（panic で落ちた thread を含む）で消える。
#[test]
fn e2e_fixture_tmp_dir_is_removed_when_the_tooth_panics() {
    let made = std::sync::Mutex::new(PathBuf::new());
    let caught = std::panic::catch_unwind(|| {
        let dir = make_tmp_dir().unwrap_or_else(|| panic!("tmp dir を作れる"));
        fs::write(dir.join("file"), "x\n").ok();
        if let Ok(mut slot) = made.lock() {
            *slot = dir.to_path_buf();
        }
        panic!("歯が落ちる");
    });
    assert!(caught.is_err(), "前提: 中で panic した");
    let path = made.lock().map(|slot| slot.clone()).unwrap_or_default();
    assert!(!path.as_os_str().is_empty(), "前提: dir を作った");
    assert!(!path.exists(), "panic の後に dir が無い: {}", path.display());
    let held = std::thread::spawn(|| {
        let dir = make_tmp_dir().unwrap_or_else(|| panic!("tmp dir を作れる")).held();
        std::panic::panic_any(dir);
    })
    .join();
    let path = held.err().and_then(|payload| payload.downcast::<PathBuf>().ok()).map(|path| *path);
    let path = path.unwrap_or_else(|| panic!("前提: 預けた thread が path を運んで panic した"));
    assert!(!path.exists(), "預けた包みも panic した thread の終端で消える: {}", path.display());
}

/// (c) guard を降ろした周は drop の後も dir が在る（降ろす口が無ければ空虚になる pin）。
#[test]
fn e2e_fixture_tmp_dir_survives_when_kept() {
    let dir = make_tmp_dir().unwrap_or_else(|| panic!("tmp dir を作れる"));
    let path = dir.keep();
    assert!(path.is_dir(), "降ろした周は dir が在る: {}", path.display());
    fs::remove_dir_all(&path).ok();
    let canonical = make_tmp_dir().and_then(TmpDir::canonical).unwrap_or_else(|| panic!("正規化できる"));
    let path = canonical.to_path_buf();
    drop(canonical);
    assert!(!path.exists(), "正規化の後も包みが生きて消える: {}", path.display());
}

/// (d) 作り手が 2 回続けて別の path を返す（既存の一意性が壊れていない）。
#[test]
fn e2e_fixture_tmp_dir_paths_are_unique() {
    let first = make_tmp_dir().unwrap_or_else(|| panic!("1 つ目を作れる"));
    let second = make_tmp_dir().unwrap_or_else(|| panic!("2 つ目を作れる"));
    assert_ne!(first.as_path(), second.as_path(), "2 回続けて別の path");
    assert!(first.is_dir() && second.is_dir(), "どちらも在る");
}

// ─────────── e2e の歯の道具箱（設計 docs/design/gate-cost.md §30・行 v・`s2-07l.504`） ───────────
//
// 歯が toy repo で実 binary を撃つときの PATH の組み立ては**この 1 関数**（[`toolbox_path`]）に寄る。
// 偽 `systemd-run` と偽 `systemctl` を置いた dir を先頭に積むので、歯の起こす toy の process は
// 実 systemd の scope を 1 本も作らない——scope は slice 直下の平面にしか作れず、実物を撃つと
// gate の箱（§4）から構造的に外れたまま user の systemd を詰まらせる（`s2-07l.504` の実測）。
// 偽にすると toy の process は歯の process の子のまま走る＝gate の箱の中に留まる。

// flip-check: retroactive s2-07l.504

/// 道具箱の偽 binary を置く dir の leaf 名（呼び手の fixture の dir の直下）。
pub const TOOLBOX_BIN: &str = "toolbox-bin";

/// 道具箱の偽 `systemd-run` が argv を写す記録 dir の leaf 名（**1 起動 1 file**）。
///
/// `tests/e2e/pipe/gate.rs` の `SCOPE_RECORDS`（明示の口の記録）とは**別の名**である——同じ置き場に
/// 重ねると、既存の歯の母集団（`scope_record(` の「ちょうど 1 件」）に道具箱の起動まで混ざる。
pub const TOOLBOX_RECORDS: &str = "toolbox-scope-args";

/// 道具箱の記録 dir（[`toolbox_path`] が作る・読み手は dir を走査する）。
pub fn toolbox_records(dir: &Path) -> PathBuf {
    dir.join(TOOLBOX_RECORDS)
}

/// 偽 `systemd-run` を `bin_dir` に 1 本書く（**偽の本体の唯一の生成元**・設計 §30 約束 3）。
///
/// argv を `<unit>.args` の**1 起動 1 file**で `records` へ写してから `--` の後ろを exec する
/// ＝包みの中身は実際に撃たれる。1 file へ追記する形にしないのは、probe の記録や別の行の記録まで
/// 同じ母集団に入り、`contains` の assert が**撃っていない起動の引数**で充足するからである。
///
/// **同じ名の 2 本目は実 systemd と同じ字面で断る**（`s2-07l.234`）——記録を上書きする形だと、
/// 同じ process が同名を 2 度撃つ周が歯に見えない。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub fn write_systemd_run_stub(bin_dir: &Path, records: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(bin_dir).expect("stub の dir を作れる");
    fs::create_dir_all(records).expect("記録の dir を作れる");
    let shim = bin_dir.join("systemd-run");
    let script = format!(
        "#!/bin/sh\n\
         __unit=no-unit\n\
         for __a in \"$@\"; do case \"$__a\" in --unit=*) __unit=${{__a#--unit=}};; esac; done\n\
         if [ -e '{0}'/\"$__unit\".args ]; then\n\
         printf 'Failed to start transient scope unit: Unit %s.scope was already loaded or has a fragment file.\\n' \"$__unit\" >&2\n\
         exit 1\n\
         fi\n\
         printf '%s\\n' \"$@\" > '{0}'/\"$__unit\".args\n\
         while [ $# -gt 0 ] && [ \"$1\" != \"--\" ]; do shift; done\n\
         shift\n\
         exec \"$@\"\n",
        records.display()
    );
    fs::write(&shim, script).expect("stub を書ける");
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).expect("stub に実行権を付ける");
}

/// 道具箱の偽 `systemctl` を `bin_dir` に 1 本書く（設計 §30 約束 4）。
///
/// `kill` は「もう無い」の字面（→ `Released::Gone`＝record に `scope=` を書かない）・`show` は空
/// （→ peak は読まない）を返す。偽が作らなかった unit に実 host が返す答えと同じなので、record の
/// field は増えも減りもしない。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_systemctl_stub(bin_dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let shim = bin_dir.join("systemctl");
    let script = "#!/bin/sh\n\
                  case \"$2\" in\n\
                  show) exit 0;;\n\
                  kill) printf 'Failed to kill unit %s: Unit %s not loaded.\\n' \"$4\" \"$4\" >&2; exit 1;;\n\
                  esac\n\
                  exit 1\n";
    fs::write(&shim, script).expect("stub を書ける");
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).expect("stub に実行権を付ける");
}

/// 道具箱の記録 dir の entry 名（昇順・dir が無ければ空＝1 件も作っていない）。
pub fn toolbox_record_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(toolbox_records(dir))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// `needle` を名に含む道具箱の記録の**ちょうど 1 件**の本文（1 行 1 引数）。
///
/// 0 件も 2 件以上も落とすのは、母集団を確かめずに `contains` すると**別の起動の引数**で assert が
/// 充足するからである（fixture 衝突・`tests/e2e/pipe/gate.rs` の `scope_record(` と同じ形）。
pub fn toolbox_record(dir: &Path, needle: &str) -> String {
    let names = toolbox_record_names(dir);
    let hits: Vec<&String> = names.iter().filter(|name| name.contains(needle)).collect();
    assert_eq!(hits.len(), 1, "{needle} の記録はちょうど 1 件（母集団 {names:?}）");
    hits.first()
        .and_then(|name| fs::read_to_string(toolbox_records(dir).join(name)).ok())
        .unwrap_or_default()
}

/// 歯が toy repo で実 binary を撃つときの PATH（設計 §30 約束 1・**3 つの口が全部ここを通る**）。
///
/// 道具箱（偽 `systemd-run` と偽 `systemctl`）を `dir` の直下に置き、その dir を**先頭に積んだ**
/// PATH の値を返す。host の PATH は後ろに残る（git / sh / cargo の解決は不変）。
pub fn toolbox_path(dir: &Path) -> String {
    let bin_dir = dir.join(TOOLBOX_BIN);
    write_systemd_run_stub(&bin_dir, &toolbox_records(dir));
    write_systemctl_stub(&bin_dir);
    format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap_or_default())
}

// ─────────────────── doctor の導入先の行（consumer-sync.md §4・AC31・`s2-07l.303`） ───────────────────

/// 導入先の歯の置き場（tmp の root・state dir・vessel repo とその HEAD・plugin root とその hooks.json の digest）。
struct ConsumerPlace {
    dir: TmpDir,
    state: PathBuf,
    vessel: PathBuf,
    head: String,
    root: PathBuf,
    digest: String,
}

/// git を 1 回撃ち、rc 0 なら stdout を返す。
fn git_out(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// commit を 1 つ持つ git repo を `dir` に作り、HEAD の sha（全桁）を返す。
fn git_repo_at(dir: &Path) -> Option<String> {
    fs::create_dir_all(dir).ok()?;
    git_out(dir, &["init", "-q"])?;
    git_out(dir, &["config", "user.name", "e2e"])?;
    git_out(dir, &["config", "user.email", "e2e@example.invalid"])?;
    fs::write(dir.join("seed"), "seed\n").ok()?;
    git_out(dir, &["add", "-A"])?;
    git_out(dir, &["commit", "-q", "-m", "seed"])?;
    git_out(dir, &["rev-parse", "HEAD"])
}

/// `hooks/hooks.json` を `body` で持つ dir を `dir` に作り、その digest を返す。
fn hooks_at(dir: &Path, body: &str) -> Option<String> {
    fs::create_dir_all(dir.join("hooks")).ok()?;
    fs::write(digest::hooks_path(dir), body).ok()?;
    digest::hooks_digest(dir)
}

/// 置き場を 1 つ作る（vessel repo は `[[vessel]]` を書く周だけ読まれる・plugin root は記録の `root=` に使う）。
fn consumer_place() -> Option<ConsumerPlace> {
    let dir = make_tmp_dir()?.canonical()?;
    let state = dir.join("state");
    fs::create_dir_all(&state).ok()?;
    let vessel = dir.join("vessel");
    let head = git_repo_at(&vessel)?;
    let root = dir.join("plugin-root");
    let digest = hooks_at(&root, "{\"hooks\":{}}\n")?;
    Some(ConsumerPlace { dir, state, vessel, head, root, digest })
}

/// host の面を書く（`[[account]]` を `labels` の順に・`vessel` が在れば `[[vessel]] repo` を 1 行）。
fn write_host(place: &ConsumerPlace, labels: &[&str], vessel: Option<&str>) {
    let mut body = "schema = 1\n".to_owned();
    for label in labels {
        body.push_str(&format!("\n[[account]]\nlabel = \"{label}\"\n"));
    }
    if let Some(repo) = vessel {
        body.push_str(&format!("\n[[vessel]]\nrepo = \"{repo}\"\n"));
    }
    fs::write(place.state.join(vessel::rules::HOST_MANIFEST), body).ok();
}

/// 帳簿の導入先 1 つ（`projectPath` / `scope` / `installPath` / `gitCommitSha`・`None` は key を書かない）。
struct LedgerRow<'a> {
    project: &'a str,
    scope: Option<&'a str>,
    install: Option<&'a str>,
    sha: Option<&'a str>,
}

/// 口座 `label` の帳簿を書く（`plugins["<NAME>@<NAME>"]` に `rows`・他の key は Claude Code の実物の形を写す）。
fn write_ledger(place: &ConsumerPlace, label: &str, rows: &[LedgerRow]) -> PathBuf {
    let items: Vec<String> = rows
        .iter()
        .map(|row| {
            let mut pairs = vec![format!("\"projectPath\":\"{}\"", row.project)];
            for (key, value) in [("scope", row.scope), ("installPath", row.install), ("gitCommitSha", row.sha)] {
                if let Some(found) = value {
                    pairs.push(format!("\"{key}\":\"{found}\""));
                }
            }
            pairs.push("\"version\":\"0.1.0\",\"installedAt\":\"2026-09-15T00:00:00Z\"".to_owned());
            format!("{{{}}}", pairs.join(","))
        })
        .collect();
    let body = format!("{{\"version\":2,\"plugins\":{{\"{NAME}@{NAME}\":[{}]}}}}", items.join(","));
    write_ledger_text(place, label, &body)
}

/// 口座 `label` の帳簿を本文そのままで書く（壊れた帳簿の周）。
fn write_ledger_text(place: &ConsumerPlace, label: &str, body: &str) -> PathBuf {
    let path = vessel::account::consumers::ledger_path(&place.state, label);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(&path, body).ok();
    path
}

/// 登録 row を 1 件積む（planner・`anchor` = 導入先の path・tmux は立てない）。
fn register_anchor(place: &ConsumerPlace, anchor: &str, target: &str) {
    let row = Registration {
        role: Role::Orchestrator,
        anchor: anchor.to_owned(),
        target: target.to_owned(),
        sid: None,
        account: "acct-1".to_owned(),
        launch: String::new(),
        model: None,
    };
    assert!(vessel::seat::role::register(&place.state, row).is_ok(), "登録 row を積める");
}

/// 席 `target` の読み込み元の記録を置く（`hooks` は digest か `None` = unreadable・`binary` は build 元 commit の字面）。
fn write_record(place: &ConsumerPlace, target: &str, root: &Path, hooks: Option<&str>, binary: &str) {
    let seat = vessel::seat::seat_dir(&place.state, target);
    fs::create_dir_all(&seat).ok();
    let record = PluginRecord::Recorded {
        root: root.display().to_string(),
        hooks: hooks.map(str::to_owned),
        binary: binary.to_owned(),
        sid: "sid-fix".to_owned(),
        ts: 1_800_000_000,
    };
    fs::write(digest::record_path(&seat), format!("{}\n", record.to_line().unwrap_or_default())).ok();
}

/// `doctor --state-dir` を撃ち（socket は server の無い path）、rc 0 と「導入先の行は口座の行の後ろ」を確かめて
/// `consumer=` の行だけを返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn consumer_lines(place: &ConsumerPlace) -> Vec<String> {
    let socket = place.dir.join("no-server-sock").display().to_string();
    let out = Command::new(env!("CARGO_BIN_EXE_scribe2"))
        .args(["doctor", "--state-dir", &place.state.display().to_string(), "--tmux-socket", &socket])
        .output()
        .expect("binary を起動できる");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(0), "doctor は判定しない（rc 0）: {stderr}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let all: Vec<&str> = stdout.lines().collect();
    let accounts = all.iter().rposition(|line| line.starts_with("account=")).unwrap_or(0);
    let first = all.iter().position(|line| line.starts_with("consumer="));
    assert!(first.is_none_or(|at| at > accounts), "導入先の行は口座の行の後ろ: {stdout}");
    all.iter().filter(|line| line.starts_with("consumer=")).map(|line| (*line).to_owned()).collect()
}

/// `head=` に載る HEAD の先頭 12 桁。
fn head12(place: &ConsumerPlace) -> String {
    place.head.chars().take(12).collect()
}

/// (d・AC31) 口座 2 つ（帳簿に導入先 2 つと 3 つ・1 つは worktree の path）と `[[vessel]] repo` を置き、登録 row の席の
/// 記録を 5 形で置く → 導入先ごとに 1 行・path の辞書順・`drift=` が `none` / `binary` / `plugin` / `ledger` / `dual` を
/// それぞれ名指す（1 行の全欄を字面で pin）。base は行が無い（RED）。
#[test]
fn doctor_consumer_lines_name_each_drift_word() {
    let place = consumer_place().unwrap_or_else(|| panic!("置き場を作れる"));
    let (head, sha12, digest) = (place.head.as_str(), head12(&place), place.digest.as_str());
    let vessel = place.vessel.display().to_string();
    write_host(&place, &["acc-a", "acc-b"], Some(&vessel));
    let cache = place.dir.join("cache");
    let cache_digest = hooks_at(&cache, "{\"hooks\":{\"Stop\":[]}}\n").unwrap_or_default();
    let (cache_s, missing) = (cache.display().to_string(), place.dir.join("no-cache").display().to_string());
    let stale = "0".repeat(40);
    write_ledger(
        &place,
        "acc-a",
        &[
            LedgerRow { project: "/c/none", scope: Some("project"), install: Some(&cache_s), sha: Some(head) },
            LedgerRow { project: "/c/binary", scope: Some("local"), install: Some(&cache_s), sha: Some(head) },
        ],
    );
    write_ledger(
        &place,
        "acc-b",
        &[
            LedgerRow { project: "/c/plugin", scope: Some("user"), install: None, sha: Some(head) },
            LedgerRow { project: "/c/ledger/.worktrees/w", scope: Some("project"), install: Some(&missing), sha: Some(&stale) },
            LedgerRow { project: "/c/dual", scope: Some("project"), install: Some(&cache_s), sha: Some(head) },
        ],
    );
    let build = env!("SCRIBE2_BUILD_COMMIT");
    let other = "f".repeat(16);
    let checkout_payload = place.vessel.join(PLUGIN_DIR);
    for (anchor, target, root, hooks, binary) in [
        ("/c/none", "n:n", &place.root, Some(digest), build),
        ("/c/binary", "b:b", &place.root, Some(digest), "000000000000"),
        ("/c/plugin", "p:p", &place.root, Some(other.as_str()), build),
        ("/c/ledger/.worktrees/w", "l:l", &place.root, Some(digest), build),
        ("/c/dual", "d:d", &checkout_payload, None, build),
    ] {
        register_anchor(&place, anchor, target);
        write_record(&place, target, root, hooks, binary);
    }
    let (root, payload) = (place.root.display().to_string(), checkout_payload.display().to_string());
    let lines = consumer_lines(&place);
    let want = [
        format!("consumer=/c/binary source=launch+install scope=local binary=000000000000 plugin={root}:{digest} ledger={head} cache={cache_digest} head={sha12} drift=binary"),
        format!("consumer=/c/dual source=launch+install scope=project binary={build} plugin={payload}:unreadable ledger={head} cache={cache_digest} head={sha12} drift=dual"),
        format!("consumer=/c/ledger/.worktrees/w source=launch+install scope=project binary={build} plugin={root}:{digest} ledger={stale} cache=absent head={sha12} drift=ledger"),
        format!("consumer=/c/none source=launch+install scope=project binary={build} plugin={root}:{digest} ledger={head} cache={cache_digest} head={sha12} drift=none"),
        format!("consumer=/c/plugin source=launch+install scope=user binary={build} plugin={root}:{other} ledger={head} cache=absent head={sha12} drift=plugin"),
    ];
    assert_eq!(lines, want, "導入先ごとに 1 行・path の辞書順・語は 1 つずつ");
    assert_ne!(cache_digest, digest, "cache は installPath の hooks.json（記録の root とは別の file）");
    fs::remove_dir_all(&place.dir).ok();
}

/// (e) 記録の無い導入先は `unrecorded`（`none` に潰さない）: 登録 row だけ（`source=launch`・`ledger=-`）・帳簿だけ
/// （`source=install`・記録の置き場が無い）・記録の位置に dir（読めない＝不在に潰さない）。帳簿の食い違いは記録が
/// 無くても測り、宣言順に `+` で繋ぐ（`ledger+unrecorded`）。
#[test]
fn doctor_consumer_unrecorded_is_not_none() {
    let place = consumer_place().unwrap_or_else(|| panic!("置き場を作れる"));
    let (head, sha12) = (place.head.as_str(), head12(&place));
    let vessel = place.vessel.display().to_string();
    write_host(&place, &["acc-a"], Some(&vessel));
    let stale = "1".repeat(40);
    write_ledger(
        &place,
        "acc-a",
        &[
            LedgerRow { project: "/u/install", scope: Some("project"), install: None, sha: Some(head) },
            LedgerRow { project: "/u/stale", scope: None, install: None, sha: Some(&stale) },
        ],
    );
    register_anchor(&place, "/u/launch", "u:launch");
    register_anchor(&place, "/u/broken", "u:broken");
    let seat = vessel::seat::seat_dir(&place.state, "u:broken");
    fs::create_dir_all(digest::record_path(&seat)).expect("記録の位置に dir を置ける");
    let lines = consumer_lines(&place);
    let tail = |source: &str, ledger: &str, drift: &str| {
        format!("source={source} scope=- binary=unrecorded plugin=unrecorded ledger={ledger} cache=absent head={sha12} drift={drift}")
    };
    let want = [
        format!("consumer=/u/broken {}", tail("launch", "-", "unrecorded")),
        format!("consumer=/u/install source=install scope=project binary=unrecorded plugin=unrecorded ledger={head} cache=absent head={sha12} drift=unrecorded"),
        format!("consumer=/u/launch {}", tail("launch", "-", "unrecorded")),
        format!("consumer=/u/stale {}", tail("install", &stale, "ledger+unrecorded")),
    ];
    assert_eq!(lines, want, "記録の無い導入先は unrecorded");
    assert!(!lines.iter().any(|line| line.ends_with("drift=none")), "none に潰さない: {lines:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (e・形 5・consumer-sync.md §17) 導入先の読み込み元は vessel repo の checkout の生成 dir（`<repo>/<PLUGIN_DIR>`）: そこに在る
/// hooks.json の digest が `plugin=<生成 dir>:<digest>` に出て、帳簿にも同じ path が在る行は `dual` と名指す。root 直下（旧 root）を
/// 記録した席は生成 dir ではないので `dual` にならず、旧 root に hooks.json が無いので `plugin` の食い違い。記録の無い導入先は
/// 従来どおり `unrecorded`（**否定の枝**）。
#[test]
fn plugin_payload_doctor_consumer_reads_the_generated_dir_under_the_checkout() {
    let place = consumer_place().unwrap_or_else(|| panic!("置き場を作れる"));
    let (head, sha12) = (place.head.as_str(), head12(&place));
    let vessel = place.vessel.display().to_string();
    write_host(&place, &["acc-a"], Some(&vessel));
    let payload = place.vessel.join(PLUGIN_DIR);
    let digest = hooks_at(&payload, "{\"hooks\":{\"PreCompact\":[]}}\n").unwrap_or_else(|| panic!("生成 dir に hooks.json を置ける"));
    assert_ne!(digest, place.digest, "生成 dir の hooks.json は plugin root の fixture と別の本文");
    write_ledger(
        &place,
        "acc-a",
        &[
            LedgerRow { project: "/p/new", scope: Some("project"), install: None, sha: Some(head) },
            LedgerRow { project: "/p/old", scope: Some("project"), install: None, sha: Some(head) },
        ],
    );
    let build = env!("SCRIBE2_BUILD_COMMIT");
    register_anchor(&place, "/p/new", "pn:pn");
    write_record(&place, "pn:pn", &payload, Some(&digest), build);
    register_anchor(&place, "/p/old", "po:po");
    write_record(&place, "po:po", &place.vessel, Some(&digest), build);
    register_anchor(&place, "/p/none", "pu:pu");
    let lines = consumer_lines(&place);
    let payload = payload.display().to_string();
    let want = [
        format!("consumer=/p/new source=launch+install scope=project binary={build} plugin={payload}:{digest} ledger={head} cache=absent head={sha12} drift=dual"),
        format!("consumer=/p/none source=launch scope=- binary=unrecorded plugin=unrecorded ledger=- cache=absent head={sha12} drift=unrecorded"),
        format!("consumer=/p/old source=launch+install scope=project binary={build} plugin={vessel}:{digest} ledger={head} cache=absent head={sha12} drift=plugin"),
    ];
    assert_eq!(lines, want, "読み込み元は生成 dir・旧 root は dual にならない・記録の無い導入先は unrecorded");
    fs::remove_dir_all(&place.dir).ok();
}

/// (f) 壊れた帳簿の口座は `ledger=unreadable` の 1 行（帳簿の path を名指す・末尾）で、他の口座の行は出る。器の key の無い
/// 帳簿・帳簿の無い口座は行 0。doctor は帳簿を書かない（bytes 不変）。
#[test]
fn doctor_consumer_survives_a_broken_ledger() {
    let place = consumer_place().unwrap_or_else(|| panic!("置き場を作れる"));
    write_host(&place, &["acc-broken", "acc-good", "acc-other", "acc-none"], None);
    let broken = write_ledger_text(&place, "acc-broken", "{\"plugins\":");
    write_ledger(&place, "acc-good", &[LedgerRow { project: "/g/one", scope: Some("project"), install: None, sha: None }]);
    write_ledger_text(&place, "acc-other", "{\"version\":2,\"plugins\":{\"other@other\":[{\"projectPath\":\"/o/x\"}]}}");
    let before = fs::read(&broken).unwrap_or_default();
    let lines = consumer_lines(&place);
    let want = [
        "consumer=/g/one source=install scope=project binary=unrecorded plugin=unrecorded ledger=- cache=absent head=undeclared drift=unrecorded".to_owned(),
        format!(
            "consumer={} source=install scope=- binary=unrecorded plugin=unrecorded ledger=unreadable cache=absent head=undeclared drift=unrecorded",
            broken.display()
        ),
    ];
    assert_eq!(lines, want, "壊れた帳簿は 1 行・他の行は出る・器の無い帳簿は行 0");
    assert_eq!(fs::read(&broken).unwrap_or_default(), before, "帳簿を書かない");
    assert!(!vessel::account::consumers::ledger_path(&place.state, "acc-none").exists(), "無い口座の帳簿を作らない");
    fs::remove_dir_all(&place.dir).ok();
}

/// (g) `[[vessel]]` が無ければ `head=undeclared`（止めない・帳簿の食い違いは測れない＝`ledger` の語は出ない）。宣言が git の
/// repo でなければ `head=unknown`。宣言が在れば同じ帳簿で `ledger` を名指す。
#[test]
fn doctor_consumer_head_is_undeclared_without_vessel_row() {
    let place = consumer_place().unwrap_or_else(|| panic!("置き場を作れる"));
    write_host(&place, &["acc-a"], None);
    let stale = "2".repeat(40);
    write_ledger(&place, "acc-a", &[LedgerRow { project: "/h/one", scope: Some("project"), install: None, sha: Some(&stale) }]);
    register_anchor(&place, "/h/one", "h:one");
    write_record(&place, "h:one", &place.root, Some(&place.digest), env!("SCRIBE2_BUILD_COMMIT"));
    let plugin = format!("{}:{}", place.root.display(), place.digest);
    let build = env!("SCRIBE2_BUILD_COMMIT");
    let line = |head: &str| {
        format!("consumer=/h/one source=launch+install scope=project binary={build} plugin={plugin} ledger={stale} cache=absent head={head} drift=none")
    };
    assert_eq!(consumer_lines(&place), [line("undeclared")], "[[vessel]] 無し");
    let not_git = place.dir.join("not-a-repo");
    fs::create_dir_all(&not_git).ok();
    write_host(&place, &["acc-a"], Some(&not_git.display().to_string()));
    assert_eq!(consumer_lines(&place), [line("unknown")], "git の repo でない宣言");
    write_host(&place, &["acc-a"], Some(&place.vessel.display().to_string()));
    let sha12 = head12(&place);
    assert_eq!(consumer_lines(&place), [line(&sha12).replace("drift=none", "drift=ledger")], "宣言が在れば帳簿の食い違いを測る");
    fs::remove_dir_all(&place.dir).ok();
}
