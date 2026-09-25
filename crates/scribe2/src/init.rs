//! 新しい repo を器に載せる口（設計 host-init.md）。本 file は行 a の `host init <TEMPLATE>`（§3）と doctor の
//! `host-template=` の 1 行を持つ。
//!
//! 雛形（既存の置き場）の在り処は git の **global** 設定 `<NAME>.template` の絶対 path ただ 1 つで、key 名は
//! NAME 定数から導く（C2.2）。**env も HOME も読まない**（global 設定の file の在り処は git が解く）。git は
//! [`Invocation`] で記述し、core は撃たない（ADR-0062）。

use crate::cli_args::{self, ArgsError};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::invocation::Invocation;
use crate::name::NAME;
use crate::rules::manifest::HostManifest;
use crate::rules::HOST_MANIFEST;
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
