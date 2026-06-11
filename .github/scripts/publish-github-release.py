#!/usr/bin/env python3
import argparse
import json
import mimetypes
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path


def github_request(method, url, token, payload=None, content_type="application/vnd.github+json"):
    data = None if payload is None else json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=data,
        method=method,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}",
            "Content-Type": content_type,
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "mini-film-release-workflow",
        },
    )
    with urllib.request.urlopen(request, timeout=180) as response:
        body = response.read()
        if not body:
            return None
        return json.loads(body.decode("utf-8"))


def github_upload(url, token, path):
    data = path.read_bytes()
    content_type = mimetypes.guess_type(path.name)[0] or "application/octet-stream"
    request = urllib.request.Request(
        url,
        data=data,
        method="POST",
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}",
            "Content-Type": content_type,
            "Content-Length": str(len(data)),
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "mini-film-release-workflow",
        },
    )
    with urllib.request.urlopen(request, timeout=300) as response:
        return json.loads(response.read().decode("utf-8"))


def get_release(repo, tag, token):
    url = f"https://api.github.com/repos/{repo}/releases/tags/{urllib.parse.quote(tag, safe='')}"
    try:
        return github_request("GET", url, token)
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return None
        raise


def list_assets(release, token):
    assets = []
    page = 1
    while True:
        url = f"{release['assets_url']}?per_page=100&page={page}"
        batch = github_request("GET", url, token)
        assets.extend(batch)
        if len(batch) < 100:
            return assets
        page += 1


def upsert_release(repo, tag, target, notes, token):
    release = get_release(repo, tag, token)
    payload = {"tag_name": tag, "target_commitish": target, "name": tag, "body": notes}
    if release is None:
        return github_request("POST", f"https://api.github.com/repos/{repo}/releases", token, payload)
    return github_request("PATCH", release["url"], token, {"name": tag, "body": notes})


def upload_assets(release, assets_dir, token):
    files = sorted(path for path in assets_dir.iterdir() if path.is_file())
    if not files:
        raise RuntimeError(f"no assets found in {assets_dir}")

    existing = {asset["name"]: asset for asset in list_assets(release, token)}
    for path in files:
        old = existing.get(path.name)
        if old:
            github_request("DELETE", old["url"], token)
        upload_base = release["upload_url"].split("{", 1)[0]
        upload_url = f"{upload_base}?{urllib.parse.urlencode({'name': path.name})}"
        github_upload(upload_url, token, path)
        print(f"uploaded {path.name}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--notes", required=True, type=Path)
    parser.add_argument("--assets", required=True, type=Path)
    args = parser.parse_args()

    token = os.environ.get("GITHUB_TOKEN")
    if not token:
        print("GITHUB_TOKEN is required", file=sys.stderr)
        return 1

    notes = args.notes.read_text(encoding="utf-8")
    release = upsert_release(args.repo, args.tag, args.target, notes, token)
    upload_assets(release, args.assets, token)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except urllib.error.HTTPError as error:
        print(f"GitHub API error {error.code}: {error.read().decode('utf-8', 'replace')}", file=sys.stderr)
        raise SystemExit(1)
    except Exception as error:
        print(error, file=sys.stderr)
        raise SystemExit(1)
