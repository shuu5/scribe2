//! tick の unit を器が導出して host へ書く `seat tick install` / `uninstall`（設計 docs/design/seat-heartbeat.md §3・契約表の
//! 行 b・ADR-0030 §2.1〜§2.5・FR64）と doctor の `tick-unit=` の 1 項目。席の起動も同じ 1 本（[`on_launch`]）を host の面の
//! `[[tick]]` の値で撃つ（設計 §5・行 d・ADR-0064）。
//!
//! 導出は pure な 1 関数（[`derive`]）: 入力は NAME・target・置き場・binary・rules の写し・周期（`seat.tick_interval_s`）だけで、
//! 同じ引数からは同じ bytes が出る＝撤去と doctor は記録でなく**導出し直した bytes** と比べる。書きは一時 file → rename・既存の
//! file は導出の bytes と比べて一致は `unchanged`・不一致は `unit-exists` で断る（人の手書きを上書きしない・N1）。有効化は
//! `systemctl --user daemon-reload` → `enable --now <timer>`、撤去は `disable --now <timer>` → 退役 dir への mv（削除しない）。
//! 置き場・binary・unit dir は全部引数で、env・home・自分の実行 file の場所は読まない（C2.2）。

use super::ROW_INTERVAL;
use crate::cli_outcome::{Outcome, RC_OK, RC_REFUSED};
use crate::fleet::store::{self, LockPolicy};
use crate::hook::{seat_name, InjectionRecord, SCHEMA};
use crate::invocation::Invocation;
use crate::name::NAME;
use crate::pipe::confine::SYSTEMCTL;
use crate::rules::manifest::{Manifest, TickUnit};
use crate::rules::{int_row, RuleError};
use crate::seat::inject::tick_path;
use crate::seat::state::now_secs;
use crate::seat::{sanitize_target, RuleRead};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Instant;

/// 記録（`tick.jsonl` の `InjectionRecord`）の `who`。
pub const WHO: &str = "seat-tick-install";
/// 撤去した unit の置き場（unit dir の直下・削除しない＝N1.2）。
pub const RETIRED_DIR: &str = ".retired";

/// 2 file の先頭行に置く器の印（`# <NAME> tick-install schema=1`）。印の無い file は器の管理物でない（`unit-foreign`）。
pub fn mark() -> String {
    format!("# {NAME} tick-install schema=1")
}

/// 導出の入力（全部を絶対 path で持つ＝unit は `WorkingDirectory=` を持たないので相対 path は解けない）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    /// `--target S:W`。
    pub target: String,
    /// 置き場（`--state-dir`）。
    pub state_dir: PathBuf,
    /// unit が撃つ binary（`--binary`）。
    pub binary: PathBuf,
    /// rules の写し（`--rules` を受けた周だけ）。
    pub rules: Option<PathBuf>,
    /// timer の周期（秒・`seat.tick_interval_s`）。
    pub interval_s: u64,
}

impl Spec {
    /// 引数の path を絶対化して組む（`std::path::absolute`＝symlink も存在も見ない・置き場の解き方と同じ）。解けない周は `None`。
    pub fn resolve(target: &str, state_dir: &Path, binary: &Path, rules: Option<&Path>, interval_s: u64) -> Option<Self> {
        let rules = match rules {
            Some(found) => Some(std::path::absolute(found).ok()?),
            None => None,
        };
        Some(Self {
            target: target.to_owned(),
            state_dir: std::path::absolute(state_dir).ok()?,
            binary: std::path::absolute(binary).ok()?,
            rules,
            interval_s,
        })
    }
}

/// 導出の結果（service と timer の file 名と本文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Units {
    /// service の file 名（`<NAME>-seat-tick-<潰した target>.service`）。
    pub service_name: String,
    /// service の本文。
    pub service: String,
    /// timer の file 名（`<NAME>-seat-tick-<潰した target>.timer`）。
    pub timer_name: String,
    /// timer の本文。
    pub timer: String,
}

impl Units {
    /// (file 名, 本文) の 2 組（service → timer の順）。
    fn files(&self) -> [(&str, &str); 2] {
        [(&self.service_name, &self.service), (&self.timer_name, &self.timer)]
    }
}

/// unit の値の 1 語を systemd の字面へ（`%` は specifier にならないよう `%%`・空白と引用符を含む語は二重引用符で包む）。
pub fn unit_word(text: &str) -> String {
    let escaped = text.replace('%', "%%");
    if escaped.chars().any(|ch| ch.is_whitespace() || matches!(ch, '"' | '\'' | '\\')) {
        format!("\"{}\"", escaped.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        escaped
    }
}

/// 導出（**pure な 1 関数**・設計 §3）: service は `Type=oneshot` の `ExecStart=<binary> seat tick --state-dir S --target S:W`
/// （`--rules` を受けた周だけ末尾に `--rules F`）・timer は単調時計の `OnBootSec` / `OnUnitActiveSec`（周期）と `Persistent=false`。
/// `Environment=` / `WorkingDirectory=` / `%h` を持たず、2 file の先頭行は器の印（[`mark`]）。
pub fn derive(spec: &Spec) -> Units {
    let stem = format!("{NAME}-seat-tick-{}", sanitize_target(&spec.target));
    let word = |path: &Path| unit_word(&path.display().to_string());
    let rules = spec.rules.as_deref().map(|found| format!(" --rules {}", word(found))).unwrap_or_default();
    let mark = mark();
    let target = unit_word(&spec.target);
    let service = format!(
        "{mark}\n[Unit]\nDescription={NAME} seat tick {target}\n\n[Service]\nType=oneshot\nExecStart={} seat tick --state-dir {} --target {target}{rules}\n",
        word(&spec.binary),
        word(&spec.state_dir)
    );
    let n = spec.interval_s;
    let timer = format!(
        "{mark}\n[Unit]\nDescription={NAME} seat tick timer {target}\n\n[Timer]\nOnBootSec={n}s\nOnUnitActiveSec={n}s\nPersistent=false\n\n[Install]\nWantedBy=timers.target\n"
    );
    Units { service_name: format!("{stem}.service"), service, timer_name: format!("{stem}.timer"), timer }
}

/// 既存の 1 file と導出の bytes の照合（**閉じた 4 値**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileState {
    /// file が無い。
    Absent,
    /// 導出の bytes と一致する。
    Same,
    /// 先頭行が器の印だが bytes が違う。
    Differs,
    /// 先頭行が器の印でない（人の手書き・読めない file を含む）。
    Foreign,
}

/// 照合（pure）: `existing` は file の bytes（無い周は `None`）。
pub fn judge_file(existing: Option<&[u8]>, want: &str) -> FileState {
    let Some(bytes) = existing else {
        return FileState::Absent;
    };
    if bytes == want.as_bytes() {
        return FileState::Same;
    }
    let first = bytes.split(|byte| *byte == b'\n').next().unwrap_or_default();
    if first == mark().as_bytes() {
        FileState::Differs
    } else {
        FileState::Foreign
    }
}

/// unit dir の 1 file を照合する（読めない周は印を確かめられない＝[`FileState::Foreign`]）。
fn state_of(path: &Path, want: &str) -> FileState {
    match fs::read(path) {
        Ok(bytes) => judge_file(Some(&bytes), want),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => FileState::Absent,
        Err(_) => FileState::Foreign,
    }
}

/// doctor の `tick-unit=` の値（**閉じた 3 値**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// 2 file が在り、印と bytes が同じ引数の導出と一致する。
    Present,
    /// 2 file とも無い。
    Absent,
    /// 在るが印が無いか bytes が違う（片方だけ在る周を含む）。
    Foreign,
}

impl Presence {
    /// `tick-unit=` の語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Absent => "absent",
            Self::Foreign => "foreign",
        }
    }
}

/// 2 file の照合を doctor の 1 値へ畳む（pure）。
pub fn presence_of(states: [FileState; 2]) -> Presence {
    match states {
        [FileState::Same, FileState::Same] => Presence::Present,
        [FileState::Absent, FileState::Absent] => Presence::Absent,
        _ => Presence::Foreign,
    }
}

/// doctor の 1 項目の入力（`--unit-dir` と `--binary`・両方がそろった周だけ在る）。
#[derive(Debug, Clone, Copy)]
pub struct Probe<'a> {
    /// `--unit-dir`。
    pub unit_dir: &'a Path,
    /// `--binary`。
    pub binary: &'a Path,
    /// `--rules`（導出の `--rules` の有無と周期の出所）。
    pub rules: Option<&'a Path>,
}

/// doctor の登録 row 1 行に足す語（`tick-unit=<present|absent|foreign>`・`systemctl` は呼ばない＝bytes で判じる）。周期の行が
/// 読めない周は導出できない＝`tick-unit=<no-rule の語>`（在るとも無いとも書かない・C10）。
pub fn doctor_word(state_dir: &Path, target: &str, probe: &Probe, manifest: &Result<Manifest, RuleRead>) -> String {
    let interval = manifest.as_ref().map_err(|failed| *failed).and_then(|found| crate::seat::int_rule_of(found, ROW_INTERVAL));
    let spec = interval.map(|n| Spec::resolve(target, state_dir, probe.binary, probe.rules, n));
    match spec {
        Ok(Some(spec)) => {
            let units = derive(&spec);
            let [service, timer] = units.files().map(|(name, want)| state_of(&probe.unit_dir.join(name), want));
            format!("tick-unit={}", presence_of([service, timer]).as_str())
        }
        Ok(None) => "tick-unit=unresolvable".to_owned(),
        Err(failed) => format!("tick-unit={}", failed.no_rule()),
    }
}

/// 断りの理由（**閉じた列**・字面は行の `reason=` の語）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// 置き場・binary・rules・unit dir の path を解けない。
    Path,
    /// `seat.tick_interval_s` が読めない（不在・不発効・整数でない・manifest が壊れている）。
    NoRule,
    /// fleet の replay が読めない。
    Store,
    /// target の登録 row が無い（器の管理外・FR40）。
    NoRow,
    /// 印は在るが導出の bytes と違う file が在る（file 名）。
    UnitExists(String),
    /// 印の無い file が在る（file 名）。
    UnitForeign(String),
    /// unit dir へ書けない・退役 dir へ移せない。
    UnitUnwritable,
    /// `daemon-reload` が落ちた（rc）。
    ReloadFailed(u8),
    /// `enable --now` が落ちた（rc）。
    EnableFailed(u8),
    /// `disable --now` が落ちた（rc）。
    DisableFailed(u8),
}

impl Refusal {
    /// 理由の語（`reason=` の値・席の起動の行の `tick-unit=refused:<語>` も同じ語）。
    pub fn word(&self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::NoRule => "no-rule",
            Self::Store => "store",
            Self::NoRow => "no-row",
            Self::UnitExists(_) => "unit-exists",
            Self::UnitForeign(_) => "unit-foreign",
            Self::UnitUnwritable => "unit-unwritable",
            Self::ReloadFailed(_) => "reload-failed",
            Self::EnableFailed(_) => "enable-failed",
            Self::DisableFailed(_) => "disable-failed",
        }
    }

    /// 行の `reason=` 以後の字面（理由の語に file 名か rc を添える）。
    pub fn render(&self) -> String {
        let detail = match self {
            Self::UnitExists(name) | Self::UnitForeign(name) => format!(" unit={name}"),
            Self::ReloadFailed(rc) | Self::EnableFailed(rc) | Self::DisableFailed(rc) => format!(" rc={rc}"),
            Self::Path | Self::NoRule | Self::Store | Self::NoRow | Self::UnitUnwritable => String::new(),
        };
        format!("reason={}{detail}", self.word())
    }
}

/// 口が解いた引数（install / uninstall の同じ形）。
pub struct Flags<'a> {
    /// `--state-dir`。
    pub state_dir: &'a str,
    /// `--target S:W`。
    pub target: &'a str,
    /// `--unit-dir`。
    pub unit_dir: &'a str,
    /// `--binary`。
    pub binary: &'a str,
    /// `--rules`。
    pub rules: Option<&'a str>,
}

/// 口の動詞（閉じた 2 値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// `seat tick install`。
    Install,
    /// `seat tick uninstall`。
    Uninstall,
}

impl Verb {
    /// 引数の字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Uninstall => "uninstall",
        }
    }

    /// 字面から読む。
    pub fn parse(token: &str) -> Option<Self> {
        [Self::Install, Self::Uninstall].into_iter().find(|verb| verb.as_str() == token)
    }
}

/// `seat tick install|uninstall` の本体: 結果の 1 行を stdout（成功）か stderr（断り・rc 1）へ。manifest が壊れている周は defect を
/// stderr へ並べる（`rules validate` と同じ字面）。
pub fn run(verb: Verb, flags: &Flags, manifest: Result<Manifest, Vec<RuleError>>) -> Outcome {
    let head = format!("seat tick {}:", verb.as_str());
    let refused = |refusal: Refusal, mut err: Vec<String>| {
        err.push(format!("{head} refused {} target={}", refusal.render(), flags.target));
        Outcome::failed(RC_REFUSED, err)
    };
    let manifest = match manifest {
        Ok(found) => found,
        Err(errors) => return refused(Refusal::NoRule, crate::rules::cli::render_defects(&errors)),
    };
    let request = Request {
        state_dir: Path::new(flags.state_dir),
        target: flags.target,
        unit_dir: Path::new(flags.unit_dir),
        binary: Path::new(flags.binary),
        rules: flags.rules.map(Path::new),
    };
    let result = prepared(&request, &manifest).and_then(|(spec, units, unit_dir)| match verb {
        Verb::Install => install(&spec, &units, &unit_dir)
            .map(|done| format!("{done} timer={} unit_dir={}", units.timer_name, unit_dir.display())),
        Verb::Uninstall => uninstall(&units, &unit_dir),
    });
    match result {
        Ok(done) => Outcome { out: vec![format!("{head} {done} target={}", flags.target)], err: Vec::new(), rc: RC_OK },
        Err(refusal) => refused(refusal, Vec::new()),
    }
}

/// 席の起動が撃つ install の 1 本（設計 seat-heartbeat.md §5 形 2・ADR-0064）: `seat tick install` と同じ門・導出・照合・書き・
/// `daemon-reload` → `enable --now` を、置き場 = 起動の置き場・target = 起動の target・unit dir と binary = host の面の
/// `[[tick]]`・rules = 起動が開いた manifest（`--rules` の写しは持たない＝unit の `ExecStart=` に `--rules` は載らない）で撃つ。
/// 返すのは起動の行の末尾に足す 1 語 `tick-unit=<installed|unchanged|refused:<理由の語>>`（断りは起動の rc を変えない）。
pub fn on_launch(state_dir: &Path, target: &str, tick: &TickUnit, manifest: &Manifest) -> String {
    let request = Request {
        state_dir,
        target,
        unit_dir: Path::new(tick.unit_dir()),
        binary: Path::new(tick.binary()),
        rules: None,
    };
    let done = prepared(&request, manifest).and_then(|(spec, units, unit_dir)| install(&spec, &units, &unit_dir));
    match done {
        Ok(word) => format!("tick-unit={word}"),
        Err(refusal) => format!("tick-unit=refused:{}", refusal.word()),
    }
}

/// install / uninstall の入力（口の flag と席の起動の同じ形・path は引数のまま＝絶対化は [`prepared`] が撃つ）。
struct Request<'a> {
    /// 置き場。
    state_dir: &'a Path,
    /// `S:W`。
    target: &'a str,
    /// unit dir。
    unit_dir: &'a Path,
    /// unit が撃つ binary。
    binary: &'a Path,
    /// rules の写し（導出の `--rules`）。
    rules: Option<&'a Path>,
}

/// 撃つ前の門を通して導出する（周期の行 → 登録 row → 導出の入力 → 導出 → unit dir の絶対化・どれかで止まる周は 1 file も触らない）。
fn prepared(request: &Request, manifest: &Manifest) -> Result<(Spec, Units, PathBuf), Refusal> {
    let spec = spec_of(request, manifest)?;
    let units = derive(&spec);
    let unit_dir = std::path::absolute(request.unit_dir).map_err(|_| Refusal::Path)?;
    Ok((spec, units, unit_dir))
}

/// 周期の行 → 登録 row → 導出の入力（撃つ前の門・どれかで止まる周は 1 file も触らない）。
fn spec_of(request: &Request, manifest: &Manifest) -> Result<Spec, Refusal> {
    let interval_s = int_row(manifest, ROW_INTERVAL).map_err(|_| Refusal::NoRule)?;
    let state_dir = std::path::absolute(request.state_dir).map_err(|_| Refusal::Path)?;
    let events = store::read_all(&state_dir).map_err(|_| Refusal::Store)?;
    let fleet = crate::fleet::replay(&events);
    crate::seat::role::registration_of_target(&fleet, request.target).ok_or(Refusal::NoRow)?;
    Spec::resolve(request.target, &state_dir, request.binary, request.rules, interval_s).ok_or(Refusal::Path)
}

/// install: 2 file を照合し、どちらかが導出と違えば 1 file も書かず `systemctl` も撃たない。無い file だけを一時 file → rename で
/// 書き、書いた周は `daemon-reload` → `enable --now`・2 file とも一致の周は `enable --now` だけ。記録は `tick.jsonl` に 1 行。
/// 返すのは結果の語（`installed` / `unchanged`）。
fn install(spec: &Spec, units: &Units, unit_dir: &Path) -> Result<&'static str, Refusal> {
    let started = Instant::now();
    let mut absent = Vec::new();
    for (name, want) in units.files() {
        match state_of(&unit_dir.join(name), want) {
            FileState::Absent => absent.push((name, want)),
            FileState::Same => {}
            FileState::Differs | FileState::Foreign => return Err(Refusal::UnitExists(name.to_owned())),
        }
    }
    fs::create_dir_all(unit_dir).map_err(|_| Refusal::UnitUnwritable)?;
    for (name, want) in &absent {
        write_unit(unit_dir, name, want).map_err(|_| Refusal::UnitUnwritable)?;
    }
    if !absent.is_empty() {
        systemctl(&["daemon-reload"]).map_err(Refusal::ReloadFailed)?;
    }
    systemctl(&["enable", "--now", &units.timer_name]).map_err(Refusal::EnableFailed)?;
    record(spec, &units.timer_name, started);
    Ok(if absent.is_empty() { "unchanged" } else { "installed" })
}

/// 1 file を一時 file → rename で書く。
fn write_unit(unit_dir: &Path, name: &str, body: &str) -> std::io::Result<()> {
    let temporary = unit_dir.join(format!(".{name}.tmp"));
    fs::write(&temporary, body)?;
    fs::rename(&temporary, unit_dir.join(name))
}

/// uninstall: 在る file のうち印の無いものは `unit-foreign`・印は在るが導出し直した bytes と違うものは `unit-exists` で断り
/// （動かさない）、2 file とも無い周は何も撃たない。それ以外は `disable --now` の後に在る file を `.retired/<name>.<UTC 秒>` へ移す。
fn uninstall(units: &Units, unit_dir: &Path) -> Result<String, Refusal> {
    let mut present = Vec::new();
    for (name, want) in units.files() {
        match state_of(&unit_dir.join(name), want) {
            FileState::Absent => {}
            FileState::Same => present.push(name),
            FileState::Differs => return Err(Refusal::UnitExists(name.to_owned())),
            FileState::Foreign => return Err(Refusal::UnitForeign(name.to_owned())),
        }
    }
    if present.is_empty() {
        return Ok(format!("absent timer={} unit_dir={}", units.timer_name, unit_dir.display()));
    }
    systemctl(&["disable", "--now", &units.timer_name]).map_err(Refusal::DisableFailed)?;
    let retired = unit_dir.join(RETIRED_DIR);
    fs::create_dir_all(&retired).map_err(|_| Refusal::UnitUnwritable)?;
    let ts = now_secs();
    for name in present {
        fs::rename(unit_dir.join(name), retired.join(format!("{name}.{ts}"))).map_err(|_| Refusal::UnitUnwritable)?;
    }
    Ok(format!("retired timer={} retired_dir={}", units.timer_name, retired.display()))
}

/// `systemctl --user <args>` を子 process で 1 回撃つ（綴りは [`SYSTEMCTL`] の 1 定数・PATH 解決）。落ちた周は rc（signal で死んだ・
/// 起動できない周は 255）。
fn systemctl(args: &[&str]) -> Result<(), u8> {
    let status = Invocation::new(SYSTEMCTL)
        .arg("--user")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(found) if found.success() => Ok(()),
        Ok(found) => Err(found.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(u8::MAX)),
        Err(_) => Err(u8::MAX),
    }
}

/// 有効化した unit を席の `tick.jsonl` に 1 行記録する（`who`=`seat-tick-install`・`what`=timer の unit 名・席の起動の記録と
/// 同じ形）。**書けない周も結果を変えない**（記録は判定そのものではない）。
fn record(spec: &Spec, unit: &str, started: Instant) {
    let entry = InjectionRecord {
        schema: SCHEMA,
        who: WHO.to_owned(),
        what: unit.to_owned(),
        when: Verb::Install.as_str().to_owned(),
        bytes: unit.len() as u64,
        // 数えていないことを 0 と書かない。
        tokens: None,
        wall_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        seat: seat_name(&spec.target),
        ts: now_secs(),
    };
    let Ok(policy) = LockPolicy::embedded() else {
        return;
    };
    let _ = store::append_line(&tick_path(&spec.state_dir, &spec.target), &entry.to_line(), policy);
}

#[cfg(test)]
mod tests {
    use super::{derive, judge_file, mark, presence_of, systemctl, unit_word, FileState, Presence, Spec, Verb};
    use crate::name::NAME;
    use crate::pipe::confine::SYSTEMCTL;
    use std::path::PathBuf;

    /// 導出の入力の fixture（絶対 path・rules なし）。
    fn spec(rules: Option<&str>) -> Spec {
        Spec {
            target: "tk:tk".to_owned(),
            state_dir: PathBuf::from("/st/state"),
            binary: PathBuf::from("/opt/bin/x"),
            rules: rules.map(PathBuf::from),
            interval_s: 60,
        }
    }

    /// 導出は fixture の逐語と一致する（service → timer・印が先頭行・周期は単調時計の 2 行）。`--rules` は受けた周だけ末尾に載る。
    #[test]
    fn seat_unit_derive_matches_the_verbatim_fixture() {
        let units = derive(&spec(None));
        assert_eq!(units.service_name, format!("{NAME}-seat-tick-tk_tk.service"));
        assert_eq!(units.timer_name, format!("{NAME}-seat-tick-tk_tk.timer"));
        assert_eq!(
            units.service,
            format!(
                "# {NAME} tick-install schema=1\n[Unit]\nDescription={NAME} seat tick tk:tk\n\n[Service]\nType=oneshot\nExecStart=/opt/bin/x seat tick --state-dir /st/state --target tk:tk\n"
            )
        );
        assert_eq!(
            units.timer,
            format!(
                "# {NAME} tick-install schema=1\n[Unit]\nDescription={NAME} seat tick timer tk:tk\n\n[Timer]\nOnBootSec=60s\nOnUnitActiveSec=60s\nPersistent=false\n\n[Install]\nWantedBy=timers.target\n"
            )
        );
        let with_rules = derive(&spec(Some("/st/rules.toml")));
        assert!(with_rules.service.ends_with(" --target tk:tk --rules /st/rules.toml\n"), "{}", with_rules.service);
        assert_eq!(with_rules.timer, units.timer, "timer は rules に依らない");
        assert_eq!(derive(&spec(None)), units, "同じ引数からは同じ bytes");
        for body in [&units.service, &units.timer] {
            for banned in ["Environment", "WorkingDirectory", "%h", "OnCalendar", "%i"] {
                assert!(!body.contains(banned), "{banned}: {body}");
            }
        }
    }

    /// unit の語: `%` は `%%`・空白と引用符を含む語は二重引用符で包む（ExecStart の語が割れず specifier にもならない）。
    #[test]
    fn seat_unit_word_escapes_specifiers_and_quotes_spaces() {
        assert_eq!(unit_word("/a/b"), "/a/b");
        assert_eq!(unit_word("/a/%h"), "/a/%%h");
        assert_eq!(unit_word("/a b/c"), "\"/a b/c\"");
        assert_eq!(unit_word("/a\"b"), "\"/a\\\"b\"");
        let spaced = derive(&Spec { state_dir: PathBuf::from("/s p/%h"), ..spec(None) });
        assert!(spaced.service.contains("--state-dir \"/s p/%%h\" --target"), "{}", spaced.service);
    }

    /// 印の判定: 無い / 一致 / 印は在るが bytes 違い / 印が無い の 4 値と、doctor の 3 値への畳み。
    #[test]
    fn seat_unit_judge_file_names_absent_same_differs_and_foreign() {
        let want = derive(&spec(None)).service;
        assert_eq!(judge_file(None, &want), FileState::Absent);
        assert_eq!(judge_file(Some(want.as_bytes()), &want), FileState::Same);
        let mut changed = want.clone().into_bytes();
        changed.push(b'\n');
        assert_eq!(judge_file(Some(&changed), &want), FileState::Differs, "1 byte 違い");
        let foreign = want.replacen(&mark(), "# hand written", 1);
        assert_eq!(judge_file(Some(foreign.as_bytes()), &want), FileState::Foreign, "印が無い");
        assert_eq!(judge_file(Some(b""), &want), FileState::Foreign, "空の file");
        assert_eq!(presence_of([FileState::Same, FileState::Same]), Presence::Present);
        assert_eq!(presence_of([FileState::Absent, FileState::Absent]), Presence::Absent);
        for states in [[FileState::Same, FileState::Absent], [FileState::Differs, FileState::Same], [FileState::Absent, FileState::Foreign]] {
            assert_eq!(presence_of(states), Presence::Foreign, "{states:?}");
        }
        let words: Vec<&str> = [Presence::Present, Presence::Absent, Presence::Foreign].iter().map(|found| found.as_str()).collect();
        assert_eq!(words, ["present", "absent", "foreign"]);
        assert_eq!([Verb::parse("install"), Verb::parse("uninstall"), Verb::parse("tick")], [Some(Verb::Install), Some(Verb::Uninstall), None]);
    }

    /// unit の有効化・撤去は起動の記述を通る（設計 core-boundary.md §9 行 j）: program は [`SYSTEMCTL`]・引数は `--user` が
    /// 先頭で args がその後の順。rc 0 は `Ok`・rc 3 は `Err(3)`・起動の失敗は `Err(255)`。
    #[test]
    fn invocation_tick_unit_systemctl_passes_user_and_args_and_refused_spawn_is_255() {
        use crate::pipe::fixture::{exited, Call, Stub};
        let stub = Stub::install(|call| match call.args.get(1).map(String::as_str) {
            Some("daemon-reload") => exited(0, b""),
            Some("enable") => exited(3, b""),
            _ => Err(std::io::Error::other("refused")),
        });
        assert_eq!(systemctl(&["daemon-reload"]), Ok(()), "rc 0");
        assert_eq!(systemctl(&["enable", "--now", "x.timer"]), Err(3), "rc 3");
        assert_eq!(systemctl(&["disable", "--now", "x.timer"]), Err(255), "起動の失敗");
        let call = |args: &[&str]| Call {
            program: SYSTEMCTL.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            cwd: None,
            envs: Vec::new(),
        };
        let expected = [
            call(&["--user", "daemon-reload"]),
            call(&["--user", "enable", "--now", "x.timer"]),
            call(&["--user", "disable", "--now", "x.timer"]),
        ];
        assert_eq!(stub.calls(), expected, "SYSTEMCTL の program と --user が先頭の引数");
    }
}
