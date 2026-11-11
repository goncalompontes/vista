from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from support import make_config

from vista_computer_use.errors import ToolError
from vista_computer_use.validation import (
    bool_value,
    check_keys,
    confirm_value,
    int_value,
    listen_value,
    output_path,
    path_value,
    pointer_value,
    resolve_point,
    resolve_point_pair,
    secret_hex_value,
    selector_params,
    target_value,
    url_value,
)


class ArgumentValidationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.config = make_config(Path(self.tmp.name))

    def assert_code(self, code: str, callable_: object, *args: object, **kwargs: object) -> None:
        with self.assertRaises(ToolError) as raised:
            callable_(*args, **kwargs)  # type: ignore[operator]
        self.assertEqual(raised.exception.code, code)

    def test_unknown_and_missing_keys(self) -> None:
        self.assert_code("invalid_arguments", check_keys, {"a": 1}, {"b"}, ())
        self.assert_code("invalid_arguments", check_keys, {}, {"a"}, ("a",))

    def test_bool_and_int_are_not_interchangeable(self) -> None:
        self.assert_code("invalid_arguments", bool_value, {"x": 1}, "x")
        self.assert_code("invalid_arguments", int_value, {"x": True}, "x")
        self.assertEqual(int_value({"x": 7}, "x", minimum=1, maximum=10), 7)
        self.assert_code("invalid_arguments", int_value, {"x": 11}, "x", maximum=10)

    def test_confirmation_must_be_explicit(self) -> None:
        self.assert_code("confirmation_required", confirm_value, {})
        self.assert_code("confirmation_required", confirm_value, {"confirm": False})
        self.assertTrue(confirm_value({"confirm": True}))

    def test_secret_hex(self) -> None:
        self.assertEqual(secret_hex_value({"s": "AB" * 32}, "s"), "ab" * 32)
        self.assert_code("invalid_arguments", secret_hex_value, {"s": "00"}, "s")
        self.assert_code("invalid_arguments", secret_hex_value, {"s": "zz" * 32}, "s")

    def test_selector_requires_one_field_and_bounds_index(self) -> None:
        self.assertEqual(
            selector_params({"text": "Continue", "match": "exact", "index": 2}),
            {"text": "Continue", "match": "exact", "index": 2},
        )
        self.assert_code("invalid_arguments", selector_params, {})
        self.assert_code("invalid_arguments", selector_params, {"text": "x", "match": "regex"})
        self.assert_code("invalid_arguments", selector_params, {"text": "x", "index": 1001})

    def test_point_validation_points_and_pixels(self) -> None:
        screen = {"width": 448, "height": 932, "scale": 3}
        self.assertEqual(resolve_point({"x": 10, "y": 20}, screen), (10, 20))
        self.assertEqual(
            resolve_point({"x": 30, "y": 60, "space": "pixels"}, screen),
            (10, 20),
        )
        # 1290x2796 is the pixel extent; the lower-right pixel is inside.
        points = resolve_point({"x": 1289, "y": 2795, "space": "pixels"}, screen)
        self.assertAlmostEqual(points[0], 1289 / 3)
        self.assertAlmostEqual(points[1], 2795 / 3)
        self.assert_code(
            "invalid_arguments", resolve_point, {"x": 448, "y": 0}, screen
        )
        first, second = resolve_point_pair(
            {"x1": 0, "y1": 0, "x2": 447, "y2": 931}, screen, "x1", "y1", "x2", "y2"
        )
        self.assertEqual(first, (0, 0))
        self.assertEqual(second, (447, 931))

    def test_output_path_is_confined(self) -> None:
        path = output_path(
            {},
            self.config,
            default_dir=self.config.screenshot_dir,
            default_name="shot",
            allowed_suffixes=[".jpg"],
        )
        self.assertEqual(path.suffix, ".jpg")
        self.assertTrue(str(path).startswith(str(self.config.screenshot_dir)))
        with self.assertRaises(ToolError) as raised:
            output_path(
                {"path": str(self.config.workspace.parent / "outside.jpg")},
                self.config,
                default_dir=self.config.screenshot_dir,
                default_name="shot",
                allowed_suffixes=[".jpg"],
            )
        self.assertEqual(raised.exception.code, "unsafe_path")

    def test_path_value_validates_suffix_and_existence(self) -> None:
        ipa = self.config.workspace / "App.ipa"
        ipa.write_bytes(b"x")
        self.assertEqual(
            path_value({"ipa_path": str(ipa)}, "ipa_path", self.config, must_exist=True, allowed_suffixes=[".ipa"]),
            ipa.resolve(),
        )
        self.assert_code(
            "invalid_arguments",
            path_value,
            {"ipa_path": str(ipa)},
            "ipa_path",
            self.config,
            allowed_suffixes=[".tipa"],
        )
        self.assert_code(
            "not_found",
            path_value,
            {"ipa_path": str(self.config.workspace / "missing.ipa")},
            "ipa_path",
            self.config,
            must_exist=True,
        )

    def test_listen_target_pointer_and_url(self) -> None:
        self.assertEqual(listen_value({}), "127.0.0.1:7443")
        self.assertEqual(listen_value({"listen": "localhost:0"}), "127.0.0.1:0")
        self.assertEqual(listen_value({"listen": "[::1]:7443"}), "[::1]:7443")
        self.assert_code("invalid_arguments", listen_value, {"listen": "example.com:7443"})
        self.assertEqual(target_value({}), "desktop")
        self.assertEqual(target_value({"target": "window:12"}), "window:12")
        self.assert_code("invalid_arguments", target_value, {"target": "screen:1"})
        self.assertEqual(pointer_value({"pointer": "0.5:0.25"}), "0.5:0.25")
        self.assert_code("invalid_arguments", pointer_value, {"pointer": "1.5:0.2"})
        self.assertEqual(url_value({"url": "https://example.com/x"}), "https://example.com/x")
        self.assert_code("invalid_arguments", url_value, {"url": "file:///etc/passwd"})


if __name__ == "__main__":
    unittest.main()
