import http.cookiejar
import importlib.util
import time
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location("uta_service", Path(__file__).resolve().parents[1] / "uta_service.py")
service = importlib.util.module_from_spec(spec)
spec.loader.exec_module(service)


def cookie(domain, name="__Secure-3PAPISID", value="test-session", expires=None):
    return http.cookiejar.Cookie(0, name, value, None, False, domain, True, domain.startswith("."),
                                 "/", True, True, expires, expires is None, None, None, {}, False)


def session():
    return {"cookies": service.encode_cookies([cookie(".youtube.com")])}


class ServiceTests(unittest.TestCase):
    def test_only_youtube_cookies_survive_and_expiry_is_respected(self):
        jar = [cookie(".youtube.com"), cookie("music.youtube.com", "SID"),
               cookie("mail.google.com"), cookie("youtube.com.evil.example"),
               cookie(".youtube.com", "expired", expires=int(time.time()) - 60)]
        rows = service.encode_cookies(jar)
        self.assertEqual({r["domain"] for r in rows}, {".youtube.com", "music.youtube.com"})
        self.assertEqual(len(rows), 2)
        headers = service.auth_headers(service.decode_cookies(rows))
        self.assertIn("__Secure-3PAPISID=test-session", headers["cookie"])
        self.assertNotIn("expired", headers["cookie"])
        self.assertEqual(headers["origin"], "https://music.youtube.com")

    def test_browser_auth_is_recognized_and_signed_by_ytmusicapi(self):
        from ytmusicapi import YTMusic
        headers = service.auth_headers(service.decode_cookies(session()["cookies"]))
        headers["X-Goog-Visitor-Id"] = "test-visitor"
        api = YTMusic(auth=headers)
        self.assertTrue(api.headers["authorization"].startswith("SAPISIDHASH "))
        self.assertNotIn("test-session", api.headers["authorization"])

    def test_invalid_or_expired_session_never_resolves_anonymously(self):
        with patch("yt_dlp.YoutubeDL") as downloader:
            with self.assertRaises(service.UserError):
                service.resolve("abcdefghijk", {"cookies": []})
            downloader.assert_not_called()

    def test_authenticated_resolution_passes_the_session_to_the_extractor(self):
        seen = {}

        class Downloader:
            def __init__(self, options):
                seen["options"] = options
                self.cookiejar = http.cookiejar.CookieJar()
            def __enter__(self): return self
            def __exit__(self, *_args): pass
            def extract_info(self, url, download):
                seen["url"] = url
                seen["cookies"] = list(self.cookiejar)
                self_test.assertFalse(download)
                return {"url": "https://media.example/audio", "acodec": "opus", "abr": 251,
                        "http_headers": {"Cookie": "must-not-leak", "Authorization": "secret",
                                         "User-Agent": "test", "Origin": "bad\r\ninjection"}}

        self_test = self
        with patch("yt_dlp.YoutubeDL", Downloader):
            result = service.resolve("abcdefghijk", session())["stream"]
        self.assertEqual(seen["cookies"][0].value, "test-session")
        self.assertEqual(seen["url"], "https://music.youtube.com/watch?v=abcdefghijk")
        self.assertTrue(result["authenticated"])
        self.assertEqual(result["headers"], {"User-Agent": "test"})
        self.assertFalse(seen["options"]["cachedir"])

    def test_anonymous_search_does_not_touch_a_keyring_or_browser(self):
        api = Mock()
        api.search.return_value = [{"videoId": "abcdefghijk", "title": "A song", "duration": "3:21"}]
        with patch.object(service, "keyring", side_effect=AssertionError("must not read keyring")), patch.object(service, "music", return_value=api):
            result = service.handle({"op": "search", "query": "song", "filter": "songs", "authenticated": False})
        self.assertEqual(result["tracks"][0]["duration"], 201)

    def test_connected_account_failure_does_not_fall_back_to_anonymous(self):
        with patch.object(service, "saved_session", side_effect=service.UserError("Keyring locked")), patch.object(service, "music") as music:
            with self.assertRaisesRegex(service.UserError, "Keyring locked"):
                service.handle({"op": "search", "query": "song", "authenticated": True})
            music.assert_not_called()

    def test_failed_connection_preserves_the_previous_saved_session(self):
        vault = Mock()
        api = Mock()
        api.get_account_info.side_effect = RuntimeError("HTTP failed")
        with patch.object(service, "keyring", return_value=vault), patch.object(service, "music", return_value=api), patch("yt_dlp.cookies.extract_cookies_from_browser", return_value=[cookie(".youtube.com")]):
            with self.assertRaises(service.UserError):
                service.connect("firefox", "")
        vault.save.assert_not_called()

    def test_locked_saved_account_remains_connected_in_status(self):
        vault = Mock()
        vault.status.return_value = {"connected": True, "account": "YouTube account"}
        vault.load.side_effect = service.UserError("Keyring locked")
        with patch.object(service, "keyring", return_value=vault):
            status = service.handle({"op": "status"})
        self.assertTrue(status["connected"])
        self.assertEqual(status["message"], "Keyring locked")

    def test_unavailable_and_malformed_songs_are_omitted(self):
        rows = service.tracks([{"videoId": "abcdefghijk", "title": "Valid", "artists": [{"name": "Artist"}], "duration": "1:02:03"},
                               {"videoId": "abcdefghij-", "isAvailable": False}, {"title": "Deleted"},
                               {"videoId": "file:///etc/passwd"}])
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["duration"], 3723)
        self.assertEqual(rows[0]["artists"], "Artist")


if __name__ == "__main__":
    unittest.main()
