#!/usr/bin/env python3
import argparse
import fnmatch
import os
import sys
import urllib.error
import urllib.request
import zipfile
from io import BytesIO
from pathlib import Path


def request_json(url, token):
    request = urllib.request.Request(
        url,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}",
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "mini-film-release-workflow",
        },
    )
    with urllib.request.urlopen(request, timeout=60) as response:
        import json

        return json.loads(response.read().decode("utf-8"))


def request_bytes(url, token):
    request = urllib.request.Request(
        url,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}",
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "mini-film-release-workflow",
        },
    )
    with urllib.request.urlopen(request, timeout=180) as response:
        return response.read()


def list_artifacts(repo, run_id, token):
    artifacts = []
    page = 1
    while True:
        url = f"https://api.github.com/repos/{repo}/actions/runs/{run_id}/artifacts?per_page=100&page={page}"
        payload = request_json(url, token)
        batch = payload.get("artifacts", [])
        artifacts.extend(batch)
        if len(artifacts) >= payload.get("total_count", 0) or not batch:
            return artifacts
        page += 1


def extract_artifact_zip(data, output):
    written = []
    with zipfile.ZipFile(BytesIO(data)) as archive:
        for member in archive.infolist():
            if member.is_dir():
                continue
            name = Path(member.filename).name
            if not name:
                continue
            target = output / name
            with archive.open(member) as source, target.open("wb") as destination:
                destination.write(source.read())
            written.append(target)
    return written


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--pattern", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    token = os.environ.get("GITHUB_TOKEN")
    if not token:
        print("GITHUB_TOKEN is required", file=sys.stderr)
        return 1

    args.output.mkdir(parents=True, exist_ok=True)
    matched = [
        artifact
        for artifact in list_artifacts(args.repo, args.run_id, token)
        if fnmatch.fnmatch(artifact.get("name", ""), args.pattern) and not artifact.get("expired", False)
    ]
    if not matched:
        print(f"no artifacts matched {args.pattern!r} for run {args.run_id}", file=sys.stderr)
        return 1

    written = []
    for artifact in matched:
        data = request_bytes(artifact["archive_download_url"], token)
        written.extend(extract_artifact_zip(data, args.output))

    for path in written:
        print(path)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except urllib.error.HTTPError as error:
        print(f"GitHub API error {error.code}: {error.read().decode('utf-8', 'replace')}", file=sys.stderr)
        raise SystemExit(1)
