"""One request on stdin, one JSON response on stdout. No credentials in argv or files.

The Rust UI owns presentation and playback. This helper isolates the unofficial
YouTube interfaces so they can be updated independently of the native toolkit.
"""

import contextlib
import http.cookiejar
import io
import json
import math
import re
import sys
import time
import urllib.request
from urllib.parse import urlsplit

APP = "org.aizome.Uta"
ATTRIBUTES = {"application": APP, "service": "youtube-music"}
BROWSERS = {"firefox", "chromium", "chrome", "brave", "edge", "vivaldi", "opera"}
VIDEO_ID = re.compile(r"[A-Za-z0-9_-]{11}\Z")
COLLECTION_ID = re.compile(r"[A-Za-z0-9_-]{1,160}\Z")


class UserError(Exception):
    """Only these deliberately written messages may be shown to the user."""


class Quiet:
    def debug(self, *_args, **_kwargs): pass
    def info(self, *_args, **_kwargs): pass
    def warning(self, *_args, **_kwargs): pass
    def error(self, *_args, **_kwargs): pass


def youtube_cookie(cookie):
    # In particular, never persist cookies from Google Mail or other sites.
    domain = cookie.domain.lstrip(".").lower()
    return (domain == "youtube.com" or domain.endswith(".youtube.com")) and not cookie.is_expired()


def encode_cookies(jar):
    return [{
        "name": c.name, "value": c.value, "domain": c.domain,
        "domain_specified": c.domain_specified, "domain_initial_dot": c.domain_initial_dot,
        "path": c.path, "secure": c.secure, "expires": c.expires,
    } for c in jar if youtube_cookie(c)]


def decode_cookies(rows):
    jar = http.cookiejar.CookieJar()
    for row in rows:
        cookie = http.cookiejar.Cookie(
            version=0, name=row["name"], value=row["value"], port=None, port_specified=False,
            domain=row["domain"], domain_specified=row.get("domain_specified", True),
            domain_initial_dot=row.get("domain_initial_dot", row["domain"].startswith(".")),
            path=row.get("path", "/"), path_specified=True, secure=row.get("secure", True),
            expires=row.get("expires"), discard=row.get("expires") is None,
            comment=None, comment_url=None, rest={}, rfc2109=False,
        )
        if youtube_cookie(cookie):
            jar.set_cookie(cookie)
    return jar


def auth_headers(jar):
    from ytmusicapi.helpers import initialize_headers
    request = urllib.request.Request("https://music.youtube.com/")
    jar.add_cookie_header(request)
    cookie = request.get_header("Cookie", "")
    if not any(c.name == "__Secure-3PAPISID" for c in jar if youtube_cookie(c)):
        raise UserError("No signed-in YouTube session was found. Sign in at music.youtube.com in this browser, then connect again.")
    # ytmusicapi identifies browser authentication by these two fields, and
    # replaces authorization with a fresh SAPISID hash for every request.
    headers = dict(initialize_headers())
    headers.update({"cookie": cookie, "authorization": "SAPISIDHASH", "x-goog-authuser": "0",
                    "origin": "https://music.youtube.com", "x-origin": "https://music.youtube.com"})
    return headers


class Keyring:
    def __init__(self, create=False):
        import secretstorage
        self.connection = secretstorage.dbus_init()
        try:
            self.collection = (secretstorage.get_default_collection(self.connection) if create
                               else secretstorage.collection.Collection(self.connection))
        except secretstorage.exceptions.ItemNotFoundException:
            self.collection = None

    def items(self):
        return list(self.collection.search_items(ATTRIBUTES)) if self.collection else []

    def status(self):
        items = self.items()
        if not items:
            return {"connected": False}
        return {"connected": True, "account": items[0].get_attributes().get("account", "YouTube account")}

    def load(self):
        items = self.items()
        if not items:
            return None
        if items[0].is_locked():
            raise UserError("Your desktop keyring is locked. Unlock it, then try again.")
        return json.loads(items[0].get_secret())

    def save(self, account, cookies):
        if self.collection.is_locked():
            self.collection.unlock()
        if self.collection.is_locked():
            raise UserError("Unlock your desktop keyring to save the YouTube session.")
        # Keep matching attributes stable so reconnect replaces the old secret.
        self.collection.create_item(
            "Uta — YouTube Music", ATTRIBUTES,
            json.dumps({"account": account, "cookies": cookies}).encode(),
            replace=True, content_type="application/json",
        )

    def delete(self):
        for item in self.items():
            if item.is_locked():
                item.unlock()
            item.delete()


def keyring(create=False):
    try:
        return Keyring(create)
    except Exception:
        raise UserError("A desktop Secret Service keyring is required to connect an account. Start or unlock your keyring, then try again.") from None


def saved_session(required=False):
    try:
        saved = keyring().load()
    except UserError:
        raise
    except Exception:
        raise UserError("Could not read the YouTube session from your keyring. Unlock it or reconnect your account.") from None
    if required and not saved:
        raise UserError("Connect your YouTube account to use your library.")
    return saved


def music(saved=None):
    from ytmusicapi import YTMusic
    if saved:
        return YTMusic(auth=auth_headers(decode_cookies(saved["cookies"])))
    return YTMusic()


def duration(value):
    if isinstance(value, (int, float)):
        return float(value) if math.isfinite(value) and value >= 0 else 0.0
    try:
        result = 0
        for part in str(value).split(":"):
            result = result * 60 + int(part)
        return max(0, result)
    except (ValueError, TypeError):
        return 0


def track(item):
    video_id = item.get("videoId") or ""
    if not VIDEO_ID.fullmatch(video_id) or item.get("isAvailable") is False:
        return None
    artists = item.get("artists") or []
    album = item.get("album") or {}
    thumbnails = item.get("thumbnails") or []
    return {
        "id": video_id, "title": item.get("title") or "Untitled song",
        "artists": ", ".join(a.get("name", "") for a in artists if isinstance(a, dict)),
        "album": album.get("name", "") if isinstance(album, dict) else str(album),
        "duration": duration(item.get("duration_seconds") or item.get("duration")),
        "thumbnail": thumbnails[-1].get("url", "") if thumbnails else "",
        "liked": item.get("likeStatus") == "LIKE",
    }


def tracks(items):
    return [row for item in items if (row := track(item)) is not None]


def collection(item, kind):
    authors = item.get("artists") or item.get("author") or []
    if isinstance(authors, dict):
        authors = [authors]
    subtitle = ", ".join(a.get("name", "") for a in authors if isinstance(a, dict))
    return {"id": item.get("browseId") if kind == "album" else item.get("playlistId"),
            "title": item.get("title", "Untitled"), "subtitle": subtitle, "kind": kind}


def connect(browser, profile):
    if browser not in BROWSERS:
        raise UserError("Choose a supported browser.")
    vault = keyring(create=True)
    from yt_dlp.cookies import extract_cookies_from_browser
    try:
        cookies = encode_cookies(extract_cookies_from_browser(browser, profile=profile or None, logger=Quiet()))
    except Exception:
        raise UserError("Could not read that browser profile. Close the browser and try again, or choose its profile directory.") from None
    session = {"cookies": cookies}
    api = music(session)
    try:
        account = api.get_account_info().get("accountName", "YouTube account")
    except Exception:
        raise UserError("YouTube did not accept this browser session. Sign in at music.youtube.com and connect again.") from None
    vault.save(account, cookies)
    return {"connected": True, "account": account}


def resolve(video_id, saved):
    if not VIDEO_ID.fullmatch(video_id):
        raise UserError("This song has an invalid YouTube ID.")
    from yt_dlp import YoutubeDL
    jar = decode_cookies(saved["cookies"]) if saved else None
    if jar is not None:
        auth_headers(jar)  # Reject an expired session before any anonymous fallback.
    options = {
        "format": "bestaudio/best", "quiet": True, "no_warnings": True,
        "logger": Quiet(), "skip_download": True, "noplaylist": True,
        "socket_timeout": 20, "retries": 1, "extractor_retries": 1,
        "cachedir": False, "js_runtimes": {"deno": {}},
    }
    with YoutubeDL(options) as downloader:
        if saved:
            # The same session used for the library MUST reach the player API.
            for cookie in jar:
                downloader.cookiejar.set_cookie(cookie)
        info = downloader.extract_info(f"https://music.youtube.com/watch?v={video_id}", download=False)
    stream_url = info.get("url", "")
    if urlsplit(stream_url).scheme != "https":
        raise UserError("YouTube did not return a playable audio stream.")
    # Only transport headers, never session cookies, reach the audio player.
    allowed = {"user-agent", "referer", "origin", "accept"}
    headers = {k: v for k, v in info.get("http_headers", {}).items()
               if k.lower() in allowed and isinstance(v, str) and "\n" not in v and "\r" not in v}
    return {"stream": {"url": stream_url, "headers": headers,
                       "codec": info.get("acodec") or "audio", "bitrate": info.get("abr") or 0,
                       "authenticated": bool(saved)}}


def handle(request):
    op = request.get("op")
    if op == "status":
        try:
            vault = keyring()
            status = vault.status()
            # The display name lives inside the encrypted secret.
            if status["connected"]:
                try:
                    status["account"] = vault.load().get("account", "YouTube account")
                except UserError as error:
                    status["message"] = str(error)
            return status
        except UserError as error:
            return {"message": str(error)}
    if op == "connect":
        return connect(request.get("browser"), request.get("profile", ""))
    if op == "disconnect":
        keyring().delete()
        return {"connected": False}

    # Anonymous use must work on desktops without a Secret Service. Rust sends
    # this flag from the current account state; there is no silent anonymous
    # fallback for a connected account whose keyring/session has failed.
    saved = saved_session(required=True) if request.get("authenticated") else None
    if op in {"liked", "playlists", "rate"} and not saved:
        raise UserError("Connect your YouTube account to use your library.")
    if op == "resolve":
        return resolve(request.get("id", ""), saved)
    api = music(saved)
    if op == "search":
        query = request.get("query", "").strip()
        search_filter = request.get("filter", "songs")
        if search_filter not in {"songs", "albums", "playlists"}:
            raise UserError("Choose songs, albums, or playlists.")
        if not query or len(query) > 500:
            raise UserError("Enter a search of between 1 and 500 characters.")
        results = api.search(query, filter=search_filter, limit=40)[:40]
        if search_filter == "songs":
            return {"tracks": tracks(results)}
        kind = "album" if search_filter == "albums" else "playlist"
        return {"collections": [row for item in results if (row := collection(item, kind))["id"]]}
    if op == "liked":
        rows = tracks(api.get_liked_songs(limit=200).get("tracks", []))[:200]
        for row in rows:
            row["liked"] = True
        return {"title": "Liked songs", "tracks": rows}
    if op == "playlists":
        return {"collections": [collection(item, "playlist") for item in api.get_library_playlists(limit=100)[:100]]}
    if op == "collection":
        identity = request.get("id", "")
        if not COLLECTION_ID.fullmatch(identity):
            raise UserError("This collection has an invalid ID.")
        if request.get("kind") == "album":
            result = api.get_album(identity)
        elif request.get("kind") == "playlist":
            result = api.get_playlist(identity, limit=200)
        else:
            raise UserError("Unknown collection type.")
        return {"title": result.get("title", "Songs"), "tracks": tracks(result.get("tracks", []))[:200]}
    if op == "rate":
        identity = request.get("id", "")
        if not VIDEO_ID.fullmatch(identity):
            raise UserError("This song has an invalid YouTube ID.")
        from ytmusicapi.models import LikeStatus
        api.rate_song(identity, LikeStatus.LIKE if request.get("like") else LikeStatus.INDIFFERENT)
        return {}
    raise UserError("Unknown music request.")


def main():
    try:
        data = sys.stdin.buffer.read(65537)
        if len(data) > 65536:
            raise UserError("Music request is too large.")
        request = json.loads(data)
        # Third-party diagnostics must never pollute the protocol or leak a
        # cookie, signed URL, or account response into application logs.
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            result = handle(request)
        response = {"data": result}
    except UserError as error:
        response = {"error": str(error)}
    except ImportError:
        response = {"error": "Music dependencies are missing. Run Uta through its Nix package or development shell."}
    except Exception:
        response = {"error": "YouTube could not complete this request. Check your connection; if signed in, reconnect your account. The service helper may need an update."}
    print(json.dumps(response, ensure_ascii=True, allow_nan=False))


if __name__ == "__main__":
    main()
