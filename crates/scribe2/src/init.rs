//! 新しい repo を器に載せる口（設計 host-init.md）。本 file は行 a の `host init <TEMPLATE>`（§3）と doctor の
//! `host-template=` の 1 行と、行 b の `init [ROOT] [--group <名>]`（§4 の 7 段）を持つ。
//!
//! 雛形（既存の置き場）の在り処は git の **global** 設定 `<NAME>.template` の絶対 path ただ 1 つで、key 名は
//! NAME 定数から導く（C2.2）。**env も HOME も読まない**（global 設定の file の在り処は git が解く）。git は
//! [`Invocation`] で記述し、core は撃たない（ADR-0062）。

use crate::account::{self, Link};
use crate::cli_args::{self, Allowed, ArgsError};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK, RC_REFUSED};
use crate::hook::vessel::{self, Bound, MARKER};
use crate::invocation::Invocation;
use crate::name::NAME;
use crate::pipe::declaration::{self, DECL_FILE};
use crate::rules::manifest::{self, HostManifest};
use crate::rules::{host_manifest_path, HOST_MANIFEST};
use std::fs;
use std::path::{Path, PathBuf};

/// 雛形の pointer を持つ git の global 設定の key（**NAME から導く**・C2.2）。
pub fn template_key() -> String {
    format!("{NAME}.template")
}

/// `host` の使い方。
pub fn host_usage() -> String {
    format!("usage: {NAME} host init <TEMPLATE>")
}

/// global 設定の雛形の pointer の読み（doctor の `host-template=` の値・**bool にしない**・C11）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Template {
    /// 設定が在り、指す dir の `host.toml` が `Absent` か `Present`（絶対 path）。
    Path(PathBuf),
    /// 設定が無い（`host init` の前）。
    Absent,
    /// git を撃てない・設定を読めない・指す先が dir でない・`host.toml` が `Unreadable`（無いに潰さない・NFR4）。
    Unreadable,
}

/// global 設定の今の値（`Ok(None)` = 設定が無い・`Err` = git を撃てない / 読めない）。
fn global_value() -> Result<Option<String>, ()> {
    let output = Invocation::new("git")
        .args(["config", "--global", "--get", &template_key()])
        .output()
        .map_err(|_| ())?;
    match output.status.code() {
        Some(0) => Ok(Some(String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())),
        // `git config --get` の rc 1 は「key が無い」だけ（壊れた file・権限は別の rc）。
        Some(1) => Ok(None),
        _ => Err(()),
    }
}

/// 雛形の dir を受けるか（dir が在り `host.toml` が `Absent` か `Present`）。断る周は理由の 1 行。
fn admit(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("{} は dir でない（無い）", dir.display()));
    }
    match HostManifest::read(&dir.join(HOST_MANIFEST)) {
        HostManifest::Absent | HostManifest::Present(_) => Ok(()),
        HostManifest::Unreadable(errors) => Err(errors
            .first()
            .map_or_else(|| format!("{HOST_MANIFEST} を読めない"), ToString::to_string)),
    }
}

/// 雛形の pointer を読む（doctor の 1 行の元・書かない）。
pub fn read_template() -> Template {
    match global_value() {
        Ok(None) => Template::Absent,
        Ok(Some(value)) if Path::new(&value).is_absolute() && admit(Path::new(&value)).is_ok() => {
            Template::Path(PathBuf::from(value))
        }
        Ok(Some(_)) | Err(()) => Template::Unreadable,
    }
}

/// doctor の 1 行（`host-template=<path|absent|unreadable>`・骨格の 2 行の直後・§3）。
pub fn render_host_template(template: &Template) -> String {
    match template {
        Template::Path(path) => format!("host-template={}", path.display()),
        Template::Absent => "host-template=absent".to_owned(),
        Template::Unreadable => "host-template=unreadable".to_owned(),
    }
}

/// doctor の 1 行を測って組む。
pub fn doctor_line() -> String {
    render_host_template(&read_template())
}

/// `host` に続く引数を捌く（verb は `init` だけ・flag を受けない）。
pub fn host_dispatch(args: &[String]) -> Outcome {
    let refused = |error: ArgsError| cli_args::refusal("host", &error, host_usage());
    let parsed = match cli_args::parse(args, &[]) {
        Ok(found) => found,
        Err(error) => return refused(error),
    };
    match parsed.positionals() {
        ["init", template] => host_init(Path::new(template)),
        ["init"] => refused(ArgsError::Missing("TEMPLATE".to_owned())),
        ["init", _, extra, ..] => refused(ArgsError::Unknown((*extra).to_owned())),
        _ => Outcome::failed(RC_REFUSED, vec![host_usage()]),
    }
}

/// `host init <TEMPLATE>`（§3）: 雛形を受ける周だけ global 設定へ絶対 path を書き、同じ値なら書かない。
///
/// 断る周（dir が無い・`host.toml` が `Unreadable`・path を解けない・今の値を読めない）は **1 byte も書かない**（fail-closed）。
fn host_init(template: &Path) -> Outcome {
    let refuse = |reason: String| Outcome::failed(RC_REFUSED, vec![format!("host: {reason}（何も書かない）")]);
    if let Err(reason) = admit(template) {
        return refuse(reason);
    }
    let absolute = match template.canonicalize() {
        Ok(found) => found,
        Err(err) => return refuse(format!("{} の絶対 path を解けない: {err}", template.display())),
    };
    let Some(value) = absolute.to_str() else {
        return refuse(format!("{} は UTF-8 でない", absolute.display()));
    };
    let current = match global_value() {
        Ok(found) => found,
        Err(()) => return refuse(format!("git の global 設定 {} を読めない", template_key())),
    };
    if current.as_deref() == Some(value) {
        return Outcome::ok_line(format!("host: init template={value} unchanged"));
    }
    let written = Invocation::new("git")
        .args(["config", "--global", &template_key(), value])
        .output()
        .is_ok_and(|out| out.status.success());
    if !written {
        return Outcome::failed(
            RC_BROKEN,
            vec![format!("host: git の global 設定 {} へ書けない", template_key())],
        );
    }
    Outcome::ok_line(format!("host: init template={value} written"))
}

/// `init` の使い方（行 b・§4）。
pub fn usage() -> String {
    format!("usage: {NAME} init [ROOT] [--group <名>]")
}

/// `init` が受ける flag（`ROOT` は positional・既定は cwd）。
const ALLOWED_INIT: &[cli_args::Allowed] = &[Allowed::value("--group")];

/// 雛形の面が無い周の新しい面の土台（`account add` と同じ `schema = 1` から作る）。
const FACE_HEAD: &str = "schema = 1\n";

/// 4 段目と 5 段目が書いた file を 1 つにする commit の題（§4 の 7）。
fn commit_message() -> String {
    format!("chore({NAME}): vessel marker and declaration")
}

/// 1 段の結果（出力の `ok|skip|failed:<理由>`・bool にしない）。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    /// 書いた。
    Wrote,
    /// 既に在る（何も書かない）。
    Skip,
    /// この段だけ書かず続きの段へ進む（理由の 1 語）。
    Failed(String),
}

impl Step {
    /// 出力の字面。
    fn render(&self) -> String {
        match self {
            Self::Wrote => "ok".to_owned(),
            Self::Skip => "skip".to_owned(),
            Self::Failed(reason) => format!("failed:{reason}"),
        }
    }
}

/// `init` に続く引数を捌く。
pub fn dispatch(args: &[String]) -> Outcome {
    let refused = |error: ArgsError| cli_args::refusal("init", &error, usage());
    let parsed = match cli_args::parse(args, ALLOWED_INIT) {
        Ok(found) => found,
        Err(error) => return refused(error),
    };
    let root = match parsed.positionals() {
        [] => match std::env::current_dir() {
            Ok(found) => found,
            Err(err) => return Outcome::failed(RC_REFUSED, vec![format!("init: cwd を解決できない: {err}（何も書かない）")]),
        },
        [root] => PathBuf::from(root),
        [_, extra, ..] => return refused(ArgsError::Unknown((*extra).to_owned())),
    };
    init(&root, parsed.value("--group"))
}

/// 新しい置き場の path（`<雛形の親>/<雛形の dir 名>-<ROOT の dir 名>`・§4 の 1）。
pub fn place_of(template: &Path, root: &Path) -> Option<PathBuf> {
    let (base, repo) = (template.file_name()?.to_str()?, root.file_name()?.to_str()?);
    Some(template.parent()?.join(format!("{base}-{repo}")))
}

/// `init [ROOT]`（§4）: 前提（git の repo・雛形の pointer）を 1 段目の前に確かめ、7 段を順に 1 行ずつ出す。
fn init(root: &Path, group: Option<&str>) -> Outcome {
    let refuse = |reason: String| Outcome::failed(RC_REFUSED, vec![format!("init: {reason}（何も書かない）")]);
    let Some(root) = vessel::repo_root(root) else {
        return refuse(format!("{} は git の repo でない", root.display()));
    };
    let template = match read_template() {
        Template::Path(found) => found,
        Template::Absent => return refuse("host-template が無い（先に host init）".to_owned()),
        Template::Unreadable => return refuse("host-template を読めない".to_owned()),
    };
    let Some(place) = place_of(&template, &root) else {
        return refuse(format!("{} から置き場の名を組めない", template.display()));
    };
    let mut written: Vec<&str> = Vec::new();
    let mut steps = vec![
        ("state-dir", make_place(&place)),
        ("host-face", inherit_face(&template, &place)),
        ("accounts", wire_accounts(&template, &place)),
    ];
    let marker = match vessel::bind(&root, &place) {
        Bound::Already => Step::Skip,
        Bound::Written => {
            written.push(MARKER);
            Step::Wrote
        }
        Bound::Other(other) => Step::Failed(format!("by-other:{other}")),
        Bound::Failed(_) => Step::Failed("write".to_owned()),
    };
    steps.push(("marker", marker));
    let decl = root.join(DECL_FILE);
    let declared = if fs::symlink_metadata(&decl).is_ok() {
        Step::Skip
    } else {
        let body = declaration::scaffold(root.join("Cargo.toml").is_file());
        fs::write(&decl, body).map_or_else(|_| Step::Failed("write".to_owned()), |()| {
            written.push(DECL_FILE);
            Step::Wrote
        })
    };
    steps.push(("declaration", declared));
    steps.push(("group", group.map_or(Step::Skip, |name| join_group(&template, &place, &root, name))));
    steps.push(("commit", commit(&root, &written)));
    let failed = steps.iter().find(|(_, step)| matches!(step, Step::Failed(_))).map(|(stage, _)| *stage);
    let mut out: Vec<String> = steps.iter().map(|(stage, step)| format!("init: {stage} {}", step.render())).collect();
    out.push(failed.map_or_else(|| "next=doctor".to_owned(), |stage| format!("next=fix:{stage}")));
    Outcome { out, err: Vec::new(), rc: if failed.is_some() { RC_REFUSED } else { RC_OK } }
}

/// 1 段目: 置き場の dir を作る（在れば skip・dir でない物が在れば failed）。
fn make_place(place: &Path) -> Step {
    match fs::symlink_metadata(place) {
        Ok(_) if place.is_dir() => Step::Skip,
        Ok(_) => Step::Failed("not-dir".to_owned()),
        Err(_) => fs::create_dir(place).map_or_else(|_| Step::Failed("write".to_owned()), |()| Step::Wrote),
    }
}

/// 本文を dir の一時 file に書き、loader で検査して通れば一時 file の path を返す（通らなければ消す）。
fn stage(dir: &Path, body: &str, also: impl Fn(&manifest::Manifest) -> bool) -> Option<PathBuf> {
    let staged = dir.join(format!("{HOST_MANIFEST}.staged"));
    if fs::write(&staged, body).is_ok() && account::face_fits(&staged, also) {
        return Some(staged);
    }
    let _ = fs::remove_file(&staged);
    None
}

/// 2 段目: 雛形の面から `[[account-group]]` を除いた写しを検査してから rename で置く（在れば 1 字も変えない）。
fn inherit_face(template: &Path, place: &Path) -> Step {
    let host = host_manifest_path(place);
    if fs::symlink_metadata(&host).is_ok() {
        return Step::Skip;
    }
    let text = match fs::read_to_string(host_manifest_path(template)) {
        Ok(found) => found,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => FACE_HEAD.to_owned(),
        Err(_) => return Step::Failed("template-unreadable".to_owned()),
    };
    match stage(place, &manifest::without_groups(&text), |_| true) {
        Some(staged) => fs::rename(&staged, &host).map_or_else(|_| Step::Failed("write".to_owned()), |()| Step::Wrote),
        None => Step::Failed("invalid".to_owned()),
    }
}

/// 雛形の面の口座 label（宣言順・面が無ければ空・読めなければ `None`）。
fn template_labels(template: &Path) -> Option<Vec<String>> {
    match HostManifest::read(&host_manifest_path(template)) {
        HostManifest::Absent => Some(Vec::new()),
        HostManifest::Present(face) => Some(face.accounts().iter().map(|found| found.label().to_owned()).collect()),
        HostManifest::Unreadable(_) => None,
    }
}

/// 3 段目: 雛形の口座の dir へ symlink で結ぶ。結ぶ先の無い label が 1 つでも在れば名指し、この段は 1 本も結ばない。
fn wire_accounts(template: &Path, place: &Path) -> Step {
    let Some(labels) = template_labels(template) else {
        return Step::Failed("template-unreadable".to_owned());
    };
    let mut missing = Vec::new();
    let mut links = Vec::new();
    for label in &labels {
        if !account::label_ok(label) {
            missing.push(label.as_str());
            continue;
        }
        match account::link_source(template, place, label) {
            Link::Present => {}
            Link::NoSource => missing.push(label.as_str()),
            Link::To(target) => links.push((label.as_str(), target)),
        }
    }
    if !missing.is_empty() {
        return Step::Failed(format!("no-source:{}", missing.join(",")));
    }
    if links.is_empty() {
        return Step::Skip;
    }
    let linked = links.iter().all(|(label, target)| account::link_account(place, label, target).is_ok());
    if linked { Step::Wrote } else { Step::Failed("write".to_owned()) }
}

/// 6 段目: 雛形と同じ親の下で群 `name` を宣言する全面の `anchors` に ROOT を足し、新しい面にも群の行を写す。全面を
/// 一時 file で検査してから rename する（1 面でも落ちれば 0 面・§9）。
fn join_group(template: &Path, place: &Path, root: &Path, name: &str) -> Step {
    let failed = |reason: String| Step::Failed(reason);
    let (Some(parent), Some(anchor)) = (template.parent(), root.to_str()) else {
        return failed("unresolvable".to_owned());
    };
    let declares = |face: &manifest::Manifest| face.groups().iter().any(|group| group.name() == name);
    if !matches!(HostManifest::read(&host_manifest_path(template)), HostManifest::Present(face) if declares(&face)) {
        return failed(format!("no-group:{name}"));
    }
    let Ok(entries) = fs::read_dir(parent) else {
        return failed("unreadable".to_owned());
    };
    let mut dirs: Vec<PathBuf> = entries.filter_map(|entry| entry.ok().map(|found| found.path())).filter(|dir| dir.is_dir()).collect();
    dirs.sort();
    let dir_name = |dir: &Path| dir.file_name().map_or_else(String::new, |found| found.to_string_lossy().into_owned());
    let (mut changes, mut block, mut place_declares) = (Vec::new(), None, false);
    for dir in &dirs {
        let host = host_manifest_path(dir);
        let face = match HostManifest::read(&host) {
            HostManifest::Absent => continue,
            HostManifest::Unreadable(_) => return failed(format!("unreadable:{}", dir_name(dir))),
            HostManifest::Present(face) => face,
        };
        if !declares(&face) {
            continue;
        }
        let edited = fs::read_to_string(&host).ok().and_then(|text| manifest::with_anchor(&text, name, anchor).map(|found| (text, found)));
        let Some((text, (body, group_block))) = edited else {
            return failed(format!("unreadable:{}", dir_name(dir)));
        };
        place_declares |= dir == place;
        if dir == template {
            block = Some(group_block);
        }
        if body != text {
            changes.push((dir.clone(), body));
        }
    }
    if !place_declares {
        let (Ok(text), Some(block)) = (fs::read_to_string(host_manifest_path(place)), block) else {
            return failed("host-face".to_owned());
        };
        let separator = if text.ends_with('\n') { "" } else { "\n" };
        changes.push((place.to_path_buf(), format!("{text}{separator}\n{}\n", block.trim_end())));
    }
    if changes.is_empty() {
        return Step::Skip;
    }
    let joined = |face: &manifest::Manifest| face.groups().iter().any(|group| group.name() == name && group.anchors().iter().any(|found| found == anchor));
    replace_all(&changes, joined).map_or_else(|dir| failed(format!("invalid:{}", dir_name(&dir))), |()| Step::Wrote)
}

/// 全面を一時 file に書いて検査し、全部通った周だけ rename する（1 面でも落ちれば一時 file を全部消し、落ちた面の dir を
/// 返す・0 面）。
fn replace_all(changes: &[(PathBuf, String)], also: impl Fn(&manifest::Manifest) -> bool + Copy) -> Result<(), PathBuf> {
    let mut staged = Vec::new();
    for (dir, body) in changes {
        let Some(found) = stage(dir, body, also) else {
            for (found, _) in &staged {
                let _ = fs::remove_file(found);
            }
            return Err(dir.clone());
        };
        staged.push((found, host_manifest_path(dir)));
    }
    // rename が落ちた面は置き場の名で名指す（検査は全部通っている）。
    staged.iter().try_for_each(|(from, to)| fs::rename(from, to).map_err(|_| to.parent().map_or_else(PathBuf::new, Path::to_path_buf)))
}

/// 7 段目: 4 段目と 5 段目が書いた file だけを `git add` と `git commit --` で 1 commit にする（0 file なら skip）。
fn commit(root: &Path, written: &[&str]) -> Step {
    if written.is_empty() {
        return Step::Skip;
    }
    let git = |head: &[&str]| {
        Invocation::new("git")
            .arg("-C")
            .arg(root)
            .args(head.iter().chain(["--"].iter()).chain(written.iter()).copied())
            .output()
            .is_ok_and(|out| out.status.success())
    };
    if git(&["add"]) && git(&["commit", "-q", "-m", &commit_message()]) {
        Step::Wrote
    } else {
        Step::Failed("git".to_owned())
    }
}
