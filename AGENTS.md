# Contributor instructions

- NEVER block the UI thread. Run blocking I/O, image decoding and resizing,
  expensive computation, and waits on background workers or asynchronous tasks.
  Async tasks must offload blocking work to a worker thread. Keep UI callbacks
  and rendering updates short; send completed results back to the UI thread.
- Follow the commit-message convention in [CONTRIBUTING.md](CONTRIBUTING.md).
  It applies to AI-generated commit messages as well as human-authored ones.
- Choose the commit type by intent and the scope by affected area. Distinguish
  application changes from documentation, repository tooling, builds, and CI.
- Keep unrelated changes separate and preserve existing contributor work.
- These conventions do not authorize committing, pushing, or rewriting history;
  perform those actions only when requested.
