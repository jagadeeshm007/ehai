# Release recipes. Needs `git-cliff` (https://git-cliff.org) on PATH.
#
#   just release 0.1.0-alpha.2
#
# Bumps the workspace version, regenerates CHANGELOG.md from conventional
# commits, commits both, and tags. Does not push — review the commit and tag,
# then `git push && git push --tags` yourself.

release version:
	#!/usr/bin/env sh
	set -eu
	if git status --porcelain | grep -q .; then
		echo "error: working tree is not clean" >&2
		exit 1
	fi
	sed -i.bak 's/^version = "[^"]*"/version = "{{ version }}"/' Cargo.toml
	rm -f Cargo.toml.bak
	cargo check --workspace >/dev/null
	git cliff --tag "v{{ version }}" --output CHANGELOG.md
	git add Cargo.toml Cargo.lock CHANGELOG.md
	git commit -m "chore(release): v{{ version }}"
	git tag -a "v{{ version }}" -m "v{{ version }}"
	echo "Tagged v{{ version }}. Review it, then: git push && git push --tags"
