# release-changelog (single feature: CHANGELOG-backed GitHub Releases)

This fixture demonstrates the release-notes boundary used by the tag-triggered
GitHub Actions release workflow. The extractor returns only the dated section
matching the supplied semantic-version tag and stops before the next release.

Run it from this demo directory through the repository script:

```bash
../../../scripts/extract-release-notes.sh v0.4.1 --root "$PWD"
```

The GitHub workflow writes the same output to a temporary notes file and passes
that file to `gh release create` with the tag name. A missing CHANGELOG section
is an error, so a tag cannot silently publish unrelated release notes.
