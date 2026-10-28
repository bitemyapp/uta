"""One request on stdin, one JSON response on stdout. No credentials in argv or files.

The Rust UI owns presentation and playback. This helper isolates the unofficial
YouTube interfaces so they can be updated independently of the native toolkit.
"""

import configparser
import contextlib
import hashlib
import http.cookiejar
import io
import json
import math
import os
import re
import shutil
import sqlite3
import sys
import tempfile
import time
import urllib.request
from urllib.parse import unquote, urlsplit

APP = "org.aizome.Uta"
ATTRIBUTES = {"application": APP, "service": "youtube-music"}
VIDEO_ID = re.compile(r"[A-Za-z0-9_-]{11}\Z")
COLLECTION_ID = re.compile(r"[A-Za-z0-9_-]{1,160}\Z")
# ytmusicapi signs every request with this cookie; YouTube's session needs the other.
SESSION = "__Secure-3PAPISID"
SIGNED_IN = {SESSION, "__Secure-3PSID"}
SONGS = 1000
FIRST_PARTY = re.compile(r"\(https,youtube\.com(,\d+)?\)")


class UserError(Exception):
    """Only these deliberately written messages may be shown to the user."""


class Quiet:
    def debug(self, *_args, **_kwargs): pass
    def info(self, *_args, **_kwargs): pass
    def warning(self, *_args, **_kwargs): pass
    def error(self, *_args, **_kwargs): pass


def youtube_host(host):
    host = host.lstrip(".").lower()
    return host == "youtube.com" or host.endswith(".youtube.com")


def youtube_cookie(cookie):
    # In particular, never persist cookies from Google Mail or other sites.
    return youtube_host(cookie.domain) and not cookie.is_expired()


def make_cookie(name, value, domain, path="/", secure=True, expires=None):
    return http.cookiejar.Cookie(
        version=0, name=name, value=value, port=None, port_specified=False,
        domain=domain, domain_specified=True, domain_initial_dot=domain.startswith("."),
        path=path or "/", path_specified=True, secure=bool(secure),
        expires=int(expires) if expires else None, discard=not expires,
        comment=None, comment_url=None, rest={}, rfc2109=False,
    )


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


def auth_headers(jar, authuser="0"):
    from ytmusicapi.helpers import initialize_headers
    request = urllib.request.Request("https://music.youtube.com/")
    jar.add_cookie_header(request)
    cookie = request.get_header("Cookie", "")
    if not any(c.name == SESSION for c in jar if youtube_cookie(c)):
        raise UserError("YouTube has signed this session out. Reconnect your account from the Account page.")
    # ytmusicapi identifies browser authentication by these two fields, and
    # replaces authorization with a fresh SAPISID hash for every request.
    headers = dict(initialize_headers())
    headers.update({"cookie": cookie, "authorization": "SAPISIDHASH", "x-goog-authuser": authuser,
                    "origin": "https://music.youtube.com", "x-origin": "https://music.youtube.com"})
    return headers


# Browsers. Discovery reads only cookie names, hosts and dates, so it never
# needs the keyring and never prompts; values are read when connecting.

def home(path):
    return os.path.expanduser(path.format(
        config=os.environ.get("XDG_CONFIG_HOME") or "~/.config",
        data=os.environ.get("XDG_DATA_HOME") or "~/.local/share"))


# Profile folders hold cookies.sqlite (Firefox's moz_cookies table).
GECKO = [
    ("Firefox", "{config}/mozilla/firefox"),
    ("Firefox", "~/.mozilla/firefox"),
    ("Firefox", "~/.var/app/org.mozilla.firefox/config/mozilla/firefox"),
    ("Firefox", "~/.var/app/org.mozilla.firefox/.mozilla/firefox"),
    ("Firefox", "~/snap/firefox/common/.mozilla/firefox"),
    ("LibreWolf", "~/.librewolf"),
    ("LibreWolf", "{config}/librewolf/librewolf"),
    ("LibreWolf", "~/.var/app/io.gitlab.librewolf-community/.librewolf"),
    ("Zen", "~/.zen"),
    ("Zen", "{config}/zen"),
    ("Zen", "~/.var/app/app.zen_browser.zen/.zen"),
    ("Floorp", "~/.floorp"),
    ("Floorp", "~/.var/app/one.ablaze.floorp/.floorp"),
    ("Waterfox", "~/.waterfox"),
    ("Waterfox", "~/.var/app/net.waterfox.waterfox/.waterfox"),
]
# User data folders hold Default, Profile 1, …, each with a Cookies database.
CHROMIUM = [
    ("Chrome", "{config}/google-chrome"),
    ("Chrome Beta", "{config}/google-chrome-beta"),
    ("Chrome Dev", "{config}/google-chrome-unstable"),
    ("Chrome", "~/.var/app/com.google.Chrome/config/google-chrome"),
    ("Chromium", "{config}/chromium"),
    ("Chromium", "~/.var/app/org.chromium.Chromium/config/chromium"),
    ("Chromium", "~/.var/app/io.github.ungoogled_software.ungoogled_chromium/config/chromium"),
    ("Chromium", "~/snap/chromium/common/chromium"),
    ("Brave", "{config}/BraveSoftware/Brave-Browser"),
    ("Brave Beta", "{config}/BraveSoftware/Brave-Browser-Beta"),
    ("Brave Nightly", "{config}/BraveSoftware/Brave-Browser-Nightly"),
    ("Brave", "~/.var/app/com.brave.Browser/config/BraveSoftware/Brave-Browser"),
    ("Edge", "{config}/microsoft-edge"),
    ("Edge Beta", "{config}/microsoft-edge-beta"),
    ("Edge Dev", "{config}/microsoft-edge-dev"),
    ("Edge", "~/.var/app/com.microsoft.Edge/config/microsoft-edge"),
    ("Vivaldi", "{config}/vivaldi"),
    ("Vivaldi Snapshot", "{config}/vivaldi-snapshot"),
    ("Vivaldi", "~/.var/app/com.vivaldi.Vivaldi/config/vivaldi"),
    ("Opera", "{config}/opera"),
    ("Opera Beta", "{config}/opera-beta"),
    ("Opera", "~/.var/app/com.opera.Opera/config/opera"),
    ("Helium", "{config}/net.imput.helium"),
    ("Thorium", "{config}/thorium"),
    ("Yandex", "{config}/yandex-browser"),
    ("Whale", "{config}/naver-whale"),
    ("qutebrowser", "{data}/qutebrowser/webengine"),
]
# libsoup keeps the same moz_cookies table in a single file.
WEBKIT = [
    ("GNOME Web", "{data}/epiphany"),
    ("GNOME Web", "~/.var/app/org.gnome.Epiphany/data/epiphany"),
]
# Firefox's own containers are named by localisation IDs, old and new.
CONTAINERS = ("Personal", "Work", "Banking", "Shopping")


def gecko_profiles(root):
    names = {}
    ini = configparser.ConfigParser(interpolation=None)
    with contextlib.suppress(Exception):
        ini.read(os.path.join(root, "profiles.ini"), encoding="utf-8")
        for section in ini.sections():
            path = ini[section].get("Path")
            if path and ini[section].get("Name"):
                relative = ini[section].get("IsRelative", "1") == "1"
                names[os.path.realpath(os.path.join(root, path) if relative else path)] = ini[section]["Name"]
    for parent in (root, os.path.join(root, "Profiles")):
        with contextlib.suppress(OSError):
            for entry in sorted(os.listdir(parent)):
                folder = os.path.join(parent, entry)
                if os.path.isfile(os.path.join(folder, "cookies.sqlite")):
                    name = names.get(os.path.realpath(folder)) or entry.partition(".")[2] or entry
                    yield folder, name


def chromium_profiles(root):
    names = {}
    with contextlib.suppress(Exception):
        with open(os.path.join(root, "Local State"), encoding="utf-8") as state:
            names = {k: v.get("name", "") for k, v in json.load(state)["profile"]["info_cache"].items()}
    folders = [("", root)]
    with contextlib.suppress(OSError):
        folders += [(e, os.path.join(root, e)) for e in sorted(os.listdir(root))
                    if e not in {"System Profile", "Guest Profile"}]
    for entry, folder in folders:
        for database in (os.path.join(folder, "Network", "Cookies"), os.path.join(folder, "Cookies")):
            if os.path.isfile(database):
                # "Your Chromium" and "Your Chrome" are the browsers' names
                # for a profile nobody named: the browser's name says it.
                name = names.get(entry) or entry
                yield database, "" if name.startswith("Your ") or name == "Default" else name
                break


def sources():
    """Every browser profile on this computer with a cookie database."""
    seen = set()
    found = []

    def add(engine, browser, profile, database):
        real = os.path.realpath(database)
        if real not in seen:
            seen.add(real)
            found.append({"engine": engine, "browser": browser, "profile": profile, "path": real})

    for browser, root in GECKO:
        for folder, profile in gecko_profiles(home(root)):
            add("gecko", browser, profile, os.path.join(folder, "cookies.sqlite"))
    for browser, root in CHROMIUM:
        for database, profile in chromium_profiles(home(root)):
            add("chromium", browser, profile, database)
    for browser, root in WEBKIT:
        database = os.path.join(home(root), "cookies.sqlite")
        if os.path.isfile(database):
            add("gecko", browser, "", database)
    return found


@contextlib.contextmanager
def snapshot(database):
    """A private copy of a live cookie database, with its write-ahead log.

    Browsers keep the database locked and a fresh sign-in is often still in
    the -wal file, which a copy of the main file alone would miss. The copy
    lives in the user's runtime folder (memory, not disk) only while it's read.
    """
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    with tempfile.TemporaryDirectory(prefix="uta-", dir=runtime if runtime and os.path.isdir(runtime) else None) as tmp:
        copy = os.path.join(tmp, "cookies.sqlite")
        shutil.copyfile(database, copy)
        for suffix in ("-wal", "-journal"):
            with contextlib.suppress(FileNotFoundError):
                shutil.copyfile(database + suffix, copy + suffix)
        connection = sqlite3.connect(copy)
        try:
            yield connection
        finally:
            connection.close()


def columns(db, table):
    return {row[1] for row in db.execute(f"PRAGMA table_info({table})")}


def seconds(value):
    """Firefox counts in microseconds or milliseconds, libsoup in seconds."""
    value = float(value or 0)
    if value > 1e14:
        return value / 1e6
    return value / 1e3 if value > 1e11 else value


def chrome_seconds(value):
    # Microseconds since 1601; zero means a session cookie.
    return value / 1e6 - 11644473600 if value else None


def container_of(origin):
    """The container a cookie belongs to, 0 for none; None if it isn't YouTube's own.

    A cookie partitioned under another site (YouTube embedded in its page) or
    from a private window is not this profile's sign-in, and could replace the
    real one in the jar. Those partitioned under youtube.com itself are sent
    to music.youtube.com, so they stay.
    """
    attributes = dict(part.partition("=")[::2] for part in (origin or "").lstrip("^").split("&") if part)
    partition = unquote(attributes.pop("partitionKey", ""))
    if set(attributes) - {"userContextId"} or (partition and not FIRST_PARTY.fullmatch(partition)):
        return None
    try:
        return int(attributes.get("userContextId", 0))
    except ValueError:
        return None


def gecko_rows(db):
    have = columns(db, "moz_cookies")
    origin = "originAttributes" if "originAttributes" in have else "''"
    used = "lastAccessed" if "lastAccessed" in have else "0"
    # Partitioned rows first, so an unpartitioned cookie of the same name wins.
    query = (f"SELECT host, name, value, path, expiry, isSecure, {origin}, {used} FROM moz_cookies "
             "WHERE host = 'youtube.com' OR host LIKE '%.youtube.com' "
             f"ORDER BY {origin} LIKE '%partitionKey=%' DESC")
    for host, name, value, path, expiry, secure, origin, used in db.execute(query):
        container = container_of(origin)
        if container is not None and youtube_host(host):
            yield container, make_cookie(name, value, host, path, secure, seconds(expiry) or None), seconds(used)


def container_names(database):
    def name(identity):
        l10n = str(identity.get("l10nId") or identity.get("l10nID") or "").lower()
        return identity.get("name") or next((n for n in CONTAINERS if n.lower() in l10n), "")
    with contextlib.suppress(Exception):
        with open(os.path.join(os.path.dirname(database), "containers.json"), encoding="utf-8") as file:
            return {c["userContextId"]: name(c) for c in json.load(file).get("identities", []) if c.get("public", True)}
    return {}


def chromium_rows(db):
    have = columns(db, "cookies")
    secure = "is_secure" if "is_secure" in have else "secure"
    used = "last_access_utc" if "last_access_utc" in have else "0"
    # Cookies partitioned under another site belong to its frame, not this
    # sign-in; those partitioned under YouTube itself come first, so an
    # unpartitioned cookie of the same name wins.
    partition, order = "", ""
    if "top_frame_site_key" in have:
        partition = "AND (top_frame_site_key IN ('', 'https://youtube.com') OR top_frame_site_key IS NULL) "
        if "has_cross_site_ancestor" in have:
            partition += "AND NOT (top_frame_site_key != '' AND has_cross_site_ancestor) "
        order = "ORDER BY top_frame_site_key != '' DESC"
    query = (f"SELECT host_key, name, value, encrypted_value, path, expires_utc, {secure}, {used} FROM cookies "
             f"WHERE (host_key = 'youtube.com' OR host_key LIKE '%.youtube.com') {partition}{order}")
    return [row for row in db.execute(query) if youtube_host(row[0])]


def signed_in(cookies):
    return SIGNED_IN <= {c.name for c in cookies if not c.is_expired()}


def inspect(source):
    """(container, signed in, last used) for each sign-in in a profile."""
    cookies, used = {0: []}, {0: 0.0}
    with snapshot(source["path"]) as db:
        if source["engine"] == "gecko":
            rows = gecko_rows(db)
        else:
            # Names and dates are not encrypted: no key is needed to look.
            rows = ((0, make_cookie(name, "", host, path, secure, chrome_seconds(expires)), chrome_seconds(when) or 0.0)
                    for host, name, _value, _sealed, path, expires, secure, when in chromium_rows(db))
        for container, cookie, when in rows:
            cookies.setdefault(container, []).append(cookie)
            used[container] = max(used.get(container, 0.0), when)
    return [(container, signed_in(cookies[container]), used[container]) for container in sorted(cookies)]


def discover():
    results = []
    for source in sources():
        try:
            summary = inspect(source)
        except Exception:
            continue
        names = container_names(source["path"]) if source["engine"] == "gecko" else {}
        for container, is_signed_in, used in summary:
            if container and not is_signed_in:
                continue
            profile = source["profile"]
            if container:
                label = names.get(container) or f"Container {container}"
                profile = f"{profile}, {label}" if profile else label
            results.append({
                "id": f"{source['engine']}:{source['path']}" + (f"#{container}" if container else ""),
                "browser": source["browser"], "profile": profile,
                "signed_in": is_signed_in, "used": used,
            })
    results.sort(key=lambda s: (not s["signed_in"], -s["used"]))
    return {"sources": results, "now": time.time()}


def derive(password):
    return hashlib.pbkdf2_hmac("sha1", password, b"saltysalt", 1, 16)


def safe_storage_passwords():
    """The keys Chromium-based browsers seal cookies with on Linux.

    Each browser keeps "<Name> Safe Storage" in the desktop keyring when it
    recognises the desktop, or is told to (Sekka starts Chromium and Chrome
    with --password-store=gnome-libsecret in Aizome). Rather than guess the
    browser's choice from the desktop's name, as yt-dlp does, try each key.
    """
    passwords = []
    try:
        import secretstorage
        connection = secretstorage.dbus_init()
    except Exception:
        connection = None
    if connection is not None:
        try:
            # Unlock only the default keyring (it asks, as the browser would);
            # never create one, and skip others that are locked.
            with contextlib.suppress(Exception):
                default = secretstorage.collection.Collection(connection)
                if default.is_locked():
                    default.unlock()
            for collection in secretstorage.get_all_collections(connection):
                with contextlib.suppress(Exception):
                    if not collection.is_locked():
                        passwords += [item.get_secret() for item in collection.get_all_items()
                                      if item.get_label().endswith(" Safe Storage")]
        except Exception:
            pass
        finally:
            with contextlib.suppress(Exception):
                connection.close()
    if "KDE" in os.environ.get("XDG_CURRENT_DESKTOP", "").split(":") and shutil.which("kwallet-query"):
        with contextlib.suppress(Exception):
            from yt_dlp.cookies import _LinuxKeyring, _get_kwallet_password
            wallet = _LinuxKeyring.KWALLET6 if os.environ.get("KDE_SESSION_VERSION") == "6" else _LinuxKeyring.KWALLET5
            for name in ("Chrome", "Chromium", "Brave", "Vivaldi", "Opera", "Microsoft Edge"):
                with contextlib.suppress(Exception):
                    if password := _get_kwallet_password(name, wallet, Quiet()):
                        passwords.append(password)
    return passwords


def decrypt(blob, host, key, hashed):
    from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
    data = blob[3:]
    if not data or len(data) % 16:
        return None
    decryptor = Cipher(algorithms.AES(key), modes.CBC(b" " * 16)).decryptor()
    plain = decryptor.update(data) + decryptor.finalize()
    pad = plain[-1]
    if not 1 <= pad <= 16 or plain[-pad:] != bytes([pad]) * pad:
        return None
    plain = plain[:-pad]
    if hashed:
        # Chrome 130 and later prefix the value with the host's hash: a wrong
        # key can't pass this check.
        if plain[:32] != hashlib.sha256(host.encode()).digest():
            return None
        plain = plain[32:]
    try:
        value = plain.decode("ascii")
    except UnicodeDecodeError:
        return None
    return value if value.isprintable() else None


def chromium_cookies(db):
    rows = chromium_rows(db)
    hashed = int(db.execute("SELECT value FROM meta WHERE key = 'version'").fetchone()[0]) >= 24
    keys = {b"v10": [derive(b"peanuts"), derive(b"")], b"v11": None}
    cookies = []
    for host, name, value, encrypted, path, expires, secure, _used in rows:
        if not value and encrypted:
            version = bytes(encrypted[:3])
            if version not in keys:
                continue
            if keys[version] is None:
                keys[version] = [derive(p) for p in safe_storage_passwords()] + [derive(b"")]
            value = None
            for i, key in enumerate(keys[version]):
                if (value := decrypt(bytes(encrypted), host, key, hashed)) is not None:
                    # Try the key that worked first for the rest.
                    keys[version].insert(0, keys[version].pop(i))
                    break
            if value is None:
                if name == SESSION:
                    raise UserError("Uta couldn't unseal this browser's cookies with the keys in your desktop keyring. Unlock the keyring, then connect again.")
                continue
        cookies.append(make_cookie(name, value, host, path, secure, chrome_seconds(expires)))
    return cookies


def read_cookies(source):
    with snapshot(source["path"]) as db:
        if source["engine"] == "gecko":
            wanted = source.get("container", 0)
            return [cookie for container, cookie, _used in gecko_rows(db) if container == wanted]
        return chromium_cookies(db)


def source_from_id(identity):
    """A source from discovery, or a folder chosen in the app."""
    if identity.startswith("folder:"):
        return folder_source(identity.removeprefix("folder:"))
    base, mark, container = identity.rpartition("#")
    if not mark or not container.isdigit():
        base, container = identity, "0"
    for source in sources():
        if f"{source['engine']}:{source['path']}" == base:
            return dict(source, container=int(container), id=identity)
    raise UserError("That browser profile is no longer available. Choose another browser.")


def folder_source(folder):
    folder = os.path.realpath(os.path.expanduser(folder))
    if not os.path.isdir(folder):
        raise UserError("Choose a browser's profile folder.")
    candidates = []
    if os.path.isfile(os.path.join(folder, "cookies.sqlite")):
        candidates.append(("gecko", os.path.join(folder, "cookies.sqlite")))
    candidates += [("chromium", database) for database, _ in chromium_profiles(folder)]
    candidates += [("gecko", os.path.join(f, "cookies.sqlite")) for f, _ in gecko_profiles(folder)
                   if os.path.join(f, "cookies.sqlite") not in {c[1] for c in candidates}]
    best = None
    for engine, database in candidates:
        source = {"engine": engine, "browser": os.path.basename(folder) or "Browser",
                  "profile": "", "path": database, "id": f"folder:{folder}"}
        with contextlib.suppress(Exception):
            for container, is_signed_in, used in inspect(source):
                if container == 0 and (best is None or (is_signed_in, used) > best[0]):
                    best = ((is_signed_in, used), source)
    if best is None:
        raise UserError("That folder has no browser cookies. Choose the profile folder of a browser you're signed in to.")
    if not best[0][0]:
        raise UserError("That browser profile isn't signed in to YouTube. Sign in at music.youtube.com there, then try again.")
    # A profile Uta also finds by itself keeps that name and identity.
    known = next((s for s in sources() if s["path"] == best[1]["path"]), None)
    return dict(known, container=0, id=f"{known['engine']}:{known['path']}") if known else best[1]


def label(source):
    return f"{source['browser']} · {source['profile']}" if source.get("profile") else source["browser"]


def pasted_cookies(text):
    """Cookies from a cookies.txt export, a request's headers or a cURL command."""
    cookies = []
    for line in text.splitlines():
        fields = line.strip("\r\n").split("\t")
        if len(fields) == 7 and fields[0]:
            domain = fields[0].removeprefix("#HttpOnly_")
            if domain.startswith("#"):
                continue
            with contextlib.suppress(ValueError):
                expires = int(fields[4] or 0)
                cookies.append(make_cookie(fields[5], fields[6], domain, fields[2],
                                           fields[3].upper() == "TRUE", expires or None))
    if cookies:
        return cookies, None
    header = None
    # cURL: -H 'cookie: …', -b '…' or --cookie "…"; headers: a cookie: line,
    # or DevTools' name and value on alternate lines.
    patterns = [r"""(?:-H|--header)\s+\$?(['"])\s*cookie\s*:\s*(?P<v>[^\n]*?)\1""",
                r"""(?:-b|--cookie)\s+\$?(['"])(?P<v>[^\n]*?)\1""",
                r"^\s*cookie\s*:\s*(?P<v>\S.*)$",
                r"^\s*cookie\s*:?[ \t]*\n\s*(?P<v>\S.*)$"]
    for pattern in patterns:
        if match := re.search(pattern, text, re.IGNORECASE | re.MULTILINE):
            header = match.group("v").strip()
            break
    if header is None and re.fullmatch(r"\s*[^=;\s]+=[^;]*(;\s*[^=;\s]+=[^;]*)*;?\s*", text):
        header = text.strip()
    for part in (header or "").split(";"):
        name, sep, value = part.strip().partition("=")
        if sep and name:
            cookies.append(make_cookie(name, value, ".youtube.com"))
    authuser = re.search(r"x-goog-authuser\s*:\s*(\d{1,2})", text, re.IGNORECASE)
    return cookies, authuser.group(1) if authuser else None


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

    def save(self, session):
        if self.collection.is_locked():
            self.collection.unlock()
        if self.collection.is_locked():
            raise UserError("Unlock your desktop keyring to save the YouTube session.")
        # Keep matching attributes stable so reconnect replaces the old secret.
        self.collection.create_item(
            "Uta — YouTube Music", ATTRIBUTES, json.dumps(session).encode(),
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
        return YTMusic(auth=auth_headers(decode_cookies(saved["cookies"]), saved.get("authuser", "0")))
    return YTMusic()


def active_account_index(jar):
    """Follow the account selected in Music, not Google's first signed-in account."""
    request = urllib.request.Request("https://music.youtube.com/", headers={
        "User-Agent": "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0",
        "Accept-Language": "en-US,en;q=0.9"})
    jar.add_cookie_header(request)
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            page = response.read(8 * 1024 * 1024).decode("utf-8", errors="replace")
    except Exception:
        raise UserError("Could not identify the active YouTube Music account. Open Music in that browser and try again.") from None
    index = re.search(r'"SESSION_INDEX"\s*:\s*"?([0-9]{1,3})"?\s*[,}]', page)
    if not index or not re.search(r'"LOGGED_IN"\s*:\s*true', page):
        raise UserError("YouTube Music did not identify a signed-in account. Open Music in that browser and try again.")
    return index.group(1)


def connect(cookies, source, authuser=None):
    """Check the cookies with YouTube, then replace the saved session."""
    vault = keyring(create=True)
    session = {"cookies": encode_cookies(cookies), "source": source}
    if not any(c["name"] == SESSION for c in session["cookies"]):
        if source.get("id"):
            raise UserError(f"{source['label']} isn't signed in to YouTube. Sign in at music.youtube.com there, then connect again.")
        raise UserError("That has no YouTube sign-in. Copy a request to music.youtube.com from your browser's developer tools, or export its cookies.txt, then try again.")
    session["authuser"] = authuser if authuser is not None else active_account_index(decode_cookies(session["cookies"]))
    try:
        session["account"] = music(session).get_account_info().get("accountName") or "YouTube account"
    except Exception:
        raise UserError("YouTube did not accept this sign-in. Open music.youtube.com in that browser to refresh it, then connect again.") from None
    vault.save(session)
    return status_of(session)


def status_of(session):
    source = session.get("source") or {}
    return {"connected": True, "account": session.get("account", "YouTube account"),
            "source": source.get("label", ""), "source_id": source.get("id", "")}


def connect_source(identity):
    source = source_from_id(identity or "")
    described = {"id": source["id"], "label": label(source)}
    try:
        cookies = read_cookies(source)
    except UserError:
        raise
    except Exception:
        raise UserError(f"Could not read {described['label']}'s cookies. Try again in a moment, or choose another browser.") from None
    return connect(cookies, described)


def connect_text(text):
    if not isinstance(text, str) or not text.strip():
        raise UserError("Copy a request to music.youtube.com or a cookies.txt export first, then try again.")
    cookies, authuser = pasted_cookies(text)
    return connect(cookies, {"id": "", "label": "Copied from your browser"}, authuser)


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


def track(item, album=None):
    video_id = item.get("videoId") or ""
    if not VIDEO_ID.fullmatch(video_id) or item.get("isAvailable") is False:
        return None
    artists = item.get("artists") or []
    # Search and playlists say "thumbnails" and "duration"; watch lists say
    # "thumbnail" and "length". Album tracks take the album's.
    thumbnails = item.get("thumbnails") or item.get("thumbnail") or (album or {}).get("thumbnails") or []
    name = item.get("album") or (album or {}).get("title") or ""
    return {
        "id": video_id, "title": item.get("title") or "Untitled song",
        "artists": ", ".join(a.get("name", "") for a in artists if isinstance(a, dict)),
        "album": name.get("name") or "" if isinstance(name, dict) else str(name),
        "duration": duration(item.get("duration_seconds") or item.get("duration") or item.get("length")),
        "thumbnail": thumbnails[-1].get("url", "") if isinstance(thumbnails, list) and thumbnails else "",
        "liked": item.get("likeStatus") == "LIKE",
    }


def tracks(items, album=None):
    return [row for item in items or [] if (row := track(item, album)) is not None]


def collection(item, kind):
    authors = item.get("artists") or item.get("author") or []
    if isinstance(authors, (dict, str)):
        authors = [authors]
    subtitle = ", ".join(a if isinstance(a, str) else a.get("name", "") for a in authors if isinstance(a, (dict, str)))
    if kind == "album" and item.get("year"):
        subtitle = f"{subtitle} · {item['year']}" if subtitle else str(item["year"])
    if kind == "album":
        identity = item.get("browseId")
    else:
        # Search gives a playlist only its browse ID: "VL" and the playlist's.
        browse = item.get("browseId") or ""
        identity = item.get("playlistId") or (browse[2:] if browse.startswith("VL") else None)
    return {"id": identity, "title": item.get("title", "Untitled"), "subtitle": subtitle, "kind": kind}


def collections(items, kind):
    return [row for item in items or [] if (row := collection(item, kind))["id"]]


def account_extractor(authuser):
    """Keep yt-dlp's bootstrap and player requests on the saved Google account."""
    from yt_dlp.extractor.youtube import YoutubeIE
    if not re.fullmatch(r"[0-9]{1,3}", authuser):
        raise UserError("Reconnect your account to refresh its account selection.")

    class SelectedAccount(YoutubeIE):
        @classmethod
        def ie_key(cls):
            return "Youtube"

        def _download_webpage(self, url, *args, **kwargs):
            if isinstance(url, str) and youtube_host(urlsplit(url).hostname or ""):
                kwargs["query"] = {**(kwargs.get("query") or {}), "authuser": authuser}
            return super()._download_webpage(url, *args, **kwargs)

        def _download_initial_webpage(self, webpage_url, webpage_client, video_id):
            page = super()._download_initial_webpage(webpage_url, webpage_client, video_id)
            config = self.extract_ytcfg(video_id, page)
            # Cookie presence alone is insufficient: Google may have revoked it.
            if config.get("LOGGED_IN") is not True or "SESSION_INDEX" not in config:
                raise UserError("YouTube has signed this session out. Reconnect your account from the Account page.")
            self._extract_session_index(config)
            return page

        def _get_requested_clients(self, url, smuggled_data, is_premium_subscriber):
            # Premium can use the fast Music client without a media PO token.
            # Other signed-in accounts need yt-dlp's broader authenticated policy.
            if is_premium_subscriber:
                return ["web_music"]
            return super()._get_requested_clients(url, smuggled_data, is_premium_subscriber)

        @staticmethod
        def _extract_session_index(*data):
            actual = YoutubeIE._extract_session_index(*data)
            if actual is not None and actual != int(authuser):
                raise UserError("YouTube selected a different account. Reconnect your account.")
            return int(authuser)

    return SelectedAccount()


def resolve(video_id, saved):
    if not VIDEO_ID.fullmatch(video_id):
        raise UserError("This song has an invalid YouTube ID.")
    from yt_dlp import YoutubeDL
    jar = decode_cookies(saved["cookies"]) if saved else None
    if jar is not None:
        auth_headers(jar)  # Reject an expired session before any anonymous fallback.
    options = {
        "format": "bestaudio/best",
        "quiet": True, "no_warnings": True,
        "logger": Quiet(), "skip_download": True, "noplaylist": True,
        "socket_timeout": 20, "retries": 1, "extractor_retries": 1,
        "cachedir": False, "js_runtimes": {"deno": {}},
    }
    with YoutubeDL(options) as downloader:
        if saved:
            downloader.add_info_extractor(account_extractor(str(saved.get("authuser", "0"))))
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
                       "authenticated": bool(saved), "format_id": str(info.get("format_id", "")),
                       "client": "yt-dlp"}}


def handle(request):
    op = request.get("op")
    if op == "status":
        try:
            vault = keyring()
            status = vault.status()
            # The display name lives inside the encrypted secret.
            if status["connected"]:
                try:
                    if saved := vault.load():
                        status.update(status_of(saved))
                except UserError as error:
                    status["message"] = str(error)
            return status
        except UserError as error:
            return {"message": str(error)}
    if op == "discover":
        return discover()
    if op == "connect":
        return connect_source(request.get("source", ""))
    if op == "connect_text":
        return connect_text(request.get("text"))
    if op == "disconnect":
        keyring().delete()
        return {"connected": False}

    # Anonymous use must work on desktops without a Secret Service. Rust sends
    # this flag from the current account state; there is no silent anonymous
    # fallback for a connected account whose keyring/session has failed.
    saved = saved_session(required=True) if request.get("authenticated") else None
    if op in {"liked", "playlists", "albums", "history", "rate"} and not saved:
        raise UserError("Connect your YouTube account to use your library.")
    if op == "native_session":
        if not saved:
            raise UserError("Connect your account before authenticated playback.")
        jar = decode_cookies(saved["cookies"])
        authuser = str(saved.get("authuser", "0"))
        if not re.fullmatch(r"[0-9]{1,3}", authuser):
            raise UserError("Reconnect your account to refresh its account selection.")
        headers = auth_headers(jar, authuser)
        expiry = min([int(time.time()) + 1800] + [c.expires for c in jar if c.expires])
        return {"credentials": {"cookie": headers["cookie"], "authuser": authuser, "expires": expiry}}
    if op == "resolve":
        return resolve(request.get("id", ""), saved)
    api = music(saved)
    if op == "search":
        query = request.get("query", "").strip()
        search_filter = request.get("filter", "top")
        if search_filter not in {"top", "songs", "videos", "albums", "playlists"}:
            raise UserError("Choose top results, songs, videos, albums, or playlists.")
        if not query or len(query) > 500:
            raise UserError("Enter a search of between 1 and 500 characters.")
        if search_filter == "top":
            # YouTube Music's own first page: the top result, then a few of
            # each kind. Mixes and uploads are videos, not catalog songs.
            results = api.search(query, limit=40)
            seen = set()
            playable = [r for r in results if r.get("resultType") in {"song", "video"}
                        and r.get("videoId") not in seen and not seen.add(r.get("videoId"))]
            return {"tracks": tracks(playable)}
        results = api.search(query, filter=search_filter, limit=40)[:40]
        if search_filter in {"songs", "videos"}:
            return {"tracks": tracks(results)}
        return {"collections": collections(results, "album" if search_filter == "albums" else "playlist")}
    if op == "liked":
        rows = tracks(api.get_liked_songs(limit=SONGS).get("tracks"))[:SONGS]
        for row in rows:
            row["liked"] = True
        return {"title": "Liked songs", "tracks": rows}
    if op == "playlists":
        return {"collections": collections(api.get_library_playlists(limit=100)[:100], "playlist")}
    if op == "albums":
        return {"collections": collections(api.get_library_albums(limit=100, order="recently_added")[:100], "album")}
    if op == "history":
        return {"title": "History", "tracks": tracks(api.get_history())[:SONGS]}
    if op == "watch":
        identity = request.get("id", "")
        if not VIDEO_ID.fullmatch(identity):
            raise UserError("This song has an invalid YouTube ID.")
        radio = bool(request.get("radio"))
        result = api.get_watch_playlist(videoId=identity, limit=50, radio=radio)
        return {"title": "Radio" if radio else "Up next", "tracks": tracks(result.get("tracks"))[:200]}
    if op == "collection":
        identity = request.get("id", "")
        if not COLLECTION_ID.fullmatch(identity):
            raise UserError("This collection has an invalid ID.")
        if request.get("kind") == "album":
            result = api.get_album(identity)
            return {"title": result.get("title", "Songs"), "tracks": tracks(result.get("tracks"), result)[:SONGS]}
        if request.get("kind") == "playlist":
            result = api.get_playlist(identity, limit=SONGS)
            return {"title": result.get("title", "Songs"), "tracks": tracks(result.get("tracks"))[:SONGS]}
        raise UserError("Unknown collection type.")
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
        data = sys.stdin.buffer.read(1024 * 1024 + 1)
        if len(data) > 1024 * 1024:
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
