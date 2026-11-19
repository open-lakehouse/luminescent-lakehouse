//! A run's artifacts, merged from their three sources.
//!
//! A PR reaches a run three ways: Omnigent detects it (`gh pr create` on the
//! runner), the executor reports it through the work-graph MCP (a PR group,
//! such as a stack's layers, in order), and `CodeHost` polling keeps it
//! current. [`merge_prs`] merges them by URL. Each source owns some fields:
//!
//! | Field | Owner |
//! |---|---|
//! | `repo`, `number`, `url` | whoever saw it first |
//! | `title`, `head`, `base` | any source that knows it |
//! | `group` | a report |
//! | `state`, `draft` | the code host once it has polled, Omnigent before |
//! | `review_decision`, `checks` | the code host |
//!
//! A source never clears a field it doesn't own, and an empty value never
//! overwrites one. `Artifact.source` is the most authoritative source that
//! has reported the PR (Omnigent, then a report, then the code host).

use factory_proto::workgraph::v1 as wg;

use crate::codehost::PrStatus;

type Source = wg::ArtifactSource;

fn rank(source: Option<Source>) -> u8 {
    match source {
        Some(Source::ARTIFACT_SOURCE_OMNIGENT) => 1,
        Some(Source::ARTIFACT_SOURCE_REPORTED) => 2,
        Some(Source::ARTIFACT_SOURCE_CODE_HOST) => 3,
        _ => 0,
    }
}

fn set(field: &mut String, value: &str) {
    if !value.is_empty() {
        value.clone_into(field);
    }
}

fn pr_mut(artifact: &mut wg::Artifact) -> Option<&mut wg::PullRequest> {
    match &mut artifact.artifact {
        Some(wg::artifact::Artifact::PullRequest(pr)) => Some(pr),
        _ => None,
    }
}

/// Merge `incoming` PRs from `source` into `run`'s artifacts, by URL. New
/// ones are appended, attributed to `step`.
pub fn merge_prs(run: &mut wg::Run, incoming: &[wg::PullRequest], source: Source, step: u32) {
    for pr in incoming.iter().filter(|p| !p.url.is_empty()) {
        let existing = run
            .artifacts
            .iter_mut()
            .find(|a| matches!(&a.artifact, Some(wg::artifact::Artifact::PullRequest(p)) if p.url == pr.url));
        let Some(artifact) = existing else {
            run.artifacts.push(wg::Artifact {
                artifact: Some(wg::artifact::Artifact::PullRequest(Box::new(pr.clone()))),
                step,
                source: source.into(),
                ..Default::default()
            });
            continue;
        };
        let current = artifact.source.as_known();
        let state_from_code_host = current == Some(Source::ARTIFACT_SOURCE_CODE_HOST);
        if rank(Some(source)) > rank(current) {
            artifact.source = source.into();
        }
        let Some(have) = pr_mut(artifact) else {
            continue;
        };
        if have.repo.is_empty() {
            set(&mut have.repo, &pr.repo);
        }
        if have.number == 0 {
            have.number = pr.number;
        }
        set(&mut have.title, &pr.title);
        set(&mut have.head, &pr.head);
        set(&mut have.base, &pr.base);
        match source {
            Source::ARTIFACT_SOURCE_CODE_HOST => {
                set(&mut have.state, &pr.state);
                have.draft = pr.draft;
                set(&mut have.review_decision, &pr.review_decision);
                set(&mut have.checks, &pr.checks);
            }
            Source::ARTIFACT_SOURCE_REPORTED => {
                if pr.group.as_option().is_some() {
                    have.group = pr.group.clone();
                }
                if !state_from_code_host {
                    set(&mut have.state, &pr.state);
                }
            }
            _ => {
                if !state_from_code_host {
                    set(&mut have.state, &pr.state);
                    have.draft = pr.draft;
                }
            }
        }
    }
}

/// A code host's status as a PR to merge.
pub fn from_status(status: &PrStatus) -> wg::PullRequest {
    wg::PullRequest {
        url: status.url.clone(),
        title: status.title.clone(),
        head: status.head.clone(),
        base: status.base.clone(),
        state: status.state.clone(),
        draft: status.draft,
        review_decision: status.review_decision.clone(),
        checks: status.checks.clone(),
        ..Default::default()
    }
}

/// Add `releases` to `run`, once each (by repo, package and version).
pub fn merge_releases(run: &mut wg::Run, releases: &[wg::Release], step: u32) {
    for release in releases {
        let seen = run.artifacts.iter().any(|a| {
            matches!(&a.artifact, Some(wg::artifact::Artifact::Release(r))
                if r.repo == release.repo && r.package == release.package && r.version == release.version)
        });
        if !seen {
            run.artifacts.push(wg::Artifact {
                artifact: Some(wg::artifact::Artifact::Release(Box::new(release.clone()))),
                step,
                source: Source::ARTIFACT_SOURCE_REPORTED.into(),
                ..Default::default()
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr(url: &str, f: impl FnOnce(&mut wg::PullRequest)) -> wg::PullRequest {
        let mut p = wg::PullRequest {
            url: url.into(),
            ..Default::default()
        };
        f(&mut p);
        p
    }

    fn only(run: &wg::Run) -> (wg::PullRequest, Option<Source>) {
        assert_eq!(run.artifacts.len(), 1, "{:?}", run.artifacts);
        let a = &run.artifacts[0];
        let Some(wg::artifact::Artifact::PullRequest(p)) = &a.artifact else {
            panic!()
        };
        ((**p).clone(), a.source.as_known())
    }

    const O: Source = Source::ARTIFACT_SOURCE_OMNIGENT;
    const R: Source = Source::ARTIFACT_SOURCE_REPORTED;
    const C: Source = Source::ARTIFACT_SOURCE_CODE_HOST;

    /// A sequence of (source, PR) merges → the PR and its source.
    #[test]
    fn precedence() {
        let u = "https://github.com/o/r/pull/1";
        let group = wg::Group {
            name: "s".into(),
            position: 1,
            of: 2,
            ..Default::default()
        };
        type Case = (
            &'static str,
            Vec<(Source, wg::PullRequest)>,
            Box<dyn Fn(&wg::PullRequest) -> bool>,
            Source,
        );
        let cases: Vec<Case> = vec![
            (
                "detected",
                vec![(
                    O,
                    pr(u, |p| {
                        p.state = "open".into();
                        p.repo = "o/r".into();
                        p.number = 1;
                    }),
                )],
                Box::new(|p| p.state == "open" && p.number == 1),
                O,
            ),
            (
                "a report adds the group",
                vec![
                    (O, pr(u, |p| p.state = "open".into())),
                    (R, pr(u, |p| p.group = group.clone().into())),
                ],
                Box::new(|p| {
                    p.state == "open" && p.group.as_option().is_some_and(|g| g.position == 1)
                }),
                R,
            ),
            (
                "the code host owns state and checks",
                vec![
                    (O, pr(u, |p| p.state = "open".into())),
                    (
                        C,
                        pr(u, |p| {
                            p.state = "merged".into();
                            p.checks = "passing".into();
                            p.review_decision = "approved".into();
                        }),
                    ),
                ],
                Box::new(|p| {
                    p.state == "merged" && p.checks == "passing" && p.review_decision == "approved"
                }),
                C,
            ),
            (
                "Omnigent doesn't undo the code host",
                vec![
                    (C, pr(u, |p| p.state = "merged".into())),
                    (O, pr(u, |p| p.state = "open".into())),
                ],
                Box::new(|p| p.state == "merged"),
                C,
            ),
            (
                "nobody else clears the group",
                vec![
                    (R, pr(u, |p| p.group = group.clone().into())),
                    (C, pr(u, |p| p.state = "open".into())),
                    (O, pr(u, |_| {})),
                ],
                Box::new(|p| p.group.as_option().is_some()),
                C,
            ),
            (
                "empty values don't overwrite",
                vec![
                    (
                        O,
                        pr(u, |p| {
                            p.title = "t".into();
                            p.head = "h".into();
                        }),
                    ),
                    (R, pr(u, |_| {})),
                ],
                Box::new(|p| p.title == "t" && p.head == "h"),
                R,
            ),
            (
                "only the code host sets checks",
                vec![
                    (O, pr(u, |p| p.checks = "passing".into())),
                    (R, pr(u, |p| p.checks = "failing".into())),
                ],
                Box::new(|p| p.checks == "passing"),
                R,
            ),
        ];
        for (name, merges, ok, source) in cases {
            let mut run = wg::Run::default();
            for (s, p) in merges {
                merge_prs(&mut run, &[p], s, 0);
            }
            let (got, got_source) = only(&run);
            assert!(ok(&got), "{name}: {got:?}");
            assert_eq!(got_source, Some(source), "{name}");
        }
    }

    #[test]
    fn new_prs_append_in_order() {
        let mut run = wg::Run::default();
        merge_prs(&mut run, &[pr("a", |_| {}), pr("b", |_| {})], R, 2);
        merge_prs(
            &mut run,
            &[pr("c", |_| {}), pr("a", |_| {}), pr("", |_| {})],
            O,
            3,
        );
        let urls: Vec<(String, u32)> = run
            .artifacts
            .iter()
            .filter_map(|a| match &a.artifact {
                Some(wg::artifact::Artifact::PullRequest(p)) => Some((p.url.clone(), a.step)),
                _ => None,
            })
            .collect();
        assert_eq!(urls, [("a".into(), 2), ("b".into(), 2), ("c".into(), 3)]);
    }

    #[test]
    fn releases_once() {
        let mut run = wg::Run::default();
        let r = wg::Release {
            repo: "o/r".into(),
            package: "p".into(),
            version: "1.0.0".into(),
            ..Default::default()
        };
        merge_releases(&mut run, &[r.clone(), r], 0);
        assert_eq!(run.artifacts.len(), 1);
    }
}
