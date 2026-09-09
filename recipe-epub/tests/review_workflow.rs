#![cfg(feature = "native")]

use recipe_epub::review::{
    ExtractionRequest, ReplayRequest, ReviewRun, RunOptions, WorkflowError, extract_to_run,
    replay_to_run,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "review-workflow-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        let fixture = Self(path.canonicalize()?);
        std::fs::write(
            fixture.path("book.epub"),
            recipe_epub_fixtures::cookbook_epub()?,
        )?;
        Ok(fixture)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    fn request(&self) -> ExtractionRequest {
        ExtractionRequest {
            book: self.path("book.epub"),
            out: self.path("run.json"),
            model: "gemini-2.5-flash".into(),
            resume: false,
            from: None,
            options: RunOptions {
                cache_dir: Some(self.path("empty-cache")),
                ..Default::default()
            },
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn extraction_owns_validation_resume_parent_and_offline_progress() -> Result {
    let fixture = Fixture::new()?;
    let request = fixture.request();
    let mut missing = request.clone();
    missing.resume = true;
    assert!(matches!(
        extract_to_run(missing, |_| {}).await,
        Err(WorkflowError::MissingRun(_))
    ));
    let mut updates = vec![];
    let outcome = extract_to_run(request.clone(), |p| updates.push(p)).await?;
    assert!(outcome.run.incomplete());
    assert_eq!(updates.len(), 2);
    assert_eq!(updates[1].total, outcome.run.chunks.len());
    assert_eq!(updates[1].completed, 0);
    let before = std::fs::read(&outcome.path)?;
    assert!(matches!(
        extract_to_run(request.clone(), |_| {}).await,
        Err(WorkflowError::OutputExists(_))
    ));
    let mut resume = request.clone();
    resume.resume = true;
    assert_eq!(
        extract_to_run(resume.clone(), |_| {}).await?.path,
        outcome.path
    );
    let checkpoint = std::fs::read(&outcome.path)?;
    let mut invalid = vec![];
    for budget in [-1.0, f64::NAN, f64::INFINITY] {
        let mut r = resume.clone();
        r.options.budget_usd = budget;
        invalid.push(r);
    }
    let mut r = resume.clone();
    r.from = Some(outcome.path.clone());
    invalid.push(r);
    let mut r = resume.clone();
    r.options.refresh = true;
    invalid.push(r);
    for r in invalid {
        let mut called = false;
        assert!(matches!(
            extract_to_run(r, |_| called = true).await,
            Err(WorkflowError::InvalidRequest(_))
        ));
        assert!(!called);
        assert_eq!(std::fs::read(&outcome.path)?, checkpoint);
    }
    resume.model = "different-model".into();
    assert!(matches!(
        extract_to_run(resume, |_| {}).await,
        Err(WorkflowError::SourceMismatch(_))
    ));
    let mut child = request;
    child.out = fixture.path("child.json");
    child.from = Some(outcome.path.clone());
    let child = extract_to_run(child, |_| {}).await?;
    assert_eq!(child.run.parent.as_deref(), outcome.path.to_str());
    assert_eq!(child.run.epub_sha256, outcome.run.epub_sha256);
    assert_eq!(std::fs::read(&outcome.path)?, before);
    Ok(())
}

#[tokio::test]
async fn replay_refreshes_same_source_and_preserves_original_evidence() -> Result {
    let fixture = Fixture::new()?;
    let original = extract_to_run(fixture.request(), |_| {}).await?;
    let before = std::fs::read(&original.path)?;
    let sidecar = original.path.with_extension("review.json");
    std::fs::write(&sidecar, b"original review evidence")?;
    let request = ReplayRequest {
        run: original.path.clone(),
        out: fixture.path("replay.json"),
        source: Some(fixture.path("book.epub")),
        image_text: None,
    };
    let replay = replay_to_run(request.clone())?;
    assert_eq!(replay.run.parent.as_deref(), original.path.to_str());
    assert_eq!(replay.run.parsed, original.run.parsed);
    assert_eq!(replay.run.epub_sha256, original.run.epub_sha256);
    assert!(matches!(
        replay_to_run(request.clone()),
        Err(WorkflowError::OutputExists(_))
    ));
    let mut bad = request.clone();
    bad.out = fixture.path("bad.json");
    let mut bytes = std::fs::read(fixture.path("book.epub"))?;
    bytes.extend_from_slice(b"different EPUB identity");
    std::fs::write(fixture.path("other.epub"), bytes)?;
    bad.source = Some(fixture.path("other.epub"));
    assert!(matches!(
        replay_to_run(bad.clone()),
        Err(WorkflowError::SourceMismatch(_))
    ));
    assert!(!bad.out.exists());
    bad.source = None;
    bad.image_text = Some(fixture.path("captions.json"));
    std::fs::write(fixture.path("captions.json"), b"invalid json")?;
    assert!(matches!(
        replay_to_run(bad),
        Err(WorkflowError::ImageText(_))
    ));
    assert_eq!(std::fs::read(&original.path)?, before);
    assert_eq!(std::fs::read(sidecar)?, b"original review evidence");
    assert!(ReviewRun::read(&replay.path)?.incomplete());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn aliases_share_canonical_identity_and_dangling_outputs_are_protected() -> Result {
    let fixture = Fixture::new()?;
    let original = extract_to_run(fixture.request(), |_| {}).await?;
    let alias = fixture.path("alias.json");
    std::os::unix::fs::symlink(&original.path, &alias)?;
    let mut resume = fixture.request();
    resume.out = alias.clone();
    resume.resume = true;
    assert_eq!(extract_to_run(resume, |_| {}).await?.path, original.path);
    assert!(std::fs::symlink_metadata(&alias)?.file_type().is_symlink());
    let replay = replay_to_run(ReplayRequest {
        run: alias,
        out: fixture.path("replay.json"),
        source: None,
        image_text: None,
    })?;
    assert_eq!(replay.run.parent.as_deref(), original.path.to_str());
    let dangling = fixture.path("dangling.json");
    std::os::unix::fs::symlink(fixture.path("missing.json"), &dangling)?;
    let mut request = fixture.request();
    request.out = dangling.clone();
    assert!(matches!(
        extract_to_run(request, |_| {}).await,
        Err(WorkflowError::OutputExists(_))
    ));
    assert!(
        std::fs::symlink_metadata(dangling)?
            .file_type()
            .is_symlink()
    );
    Ok(())
}
