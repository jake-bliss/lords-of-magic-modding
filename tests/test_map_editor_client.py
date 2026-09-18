"""The map editor's browser client, driven through its own event handlers.

The client decides **where a paint lands**. Every Rust test drives `Editor::handle` directly, so
until this file the pixel-to-cell mapping was covered by reading the code and nothing else -- and
the reviewer who read it was right that it is correct today. That is exactly the problem: the
mapping depends on the stylesheet setting no CSS size on the canvases, so a future CSS change
breaks it invisibly with every other test green.

`tools/map_editor_client_harness.js` loads `src/ui/app.js` verbatim -- the same bytes the binary
serves -- against a stub DOM, fires the real handlers, and reports what the client computed. The
expected values live here, next to the reasoning for them.

Node is a prerequisite for this file and a missing one is a **failure**, not a skip: a check that
quietly does not run is not a check, and three verifiers in this repository have already shipped
unable to fail.
"""

import json
import re
import shutil
import os
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HARNESS = ROOT / "tools" / "map_editor_client_harness.js"
APP_JS = ROOT / "spikes" / "asset-viewer" / "src" / "ui" / "app.js"
STYLE_CSS = ROOT / "spikes" / "asset-viewer" / "src" / "ui" / "style.css"


def run_harness(platform=None):
    node = shutil.which("node")
    if node is None:
        raise AssertionError(
            "node is required to test the map editor's client and was not found on PATH. "
            "Install it (brew install node) rather than skipping: an unrunnable check is not a "
            "check."
        )
    environment = dict(os.environ)
    if platform is not None:
        environment["LOM_TEST_PLATFORM"] = platform
    finished = subprocess.run(
        [node, str(HARNESS), str(APP_JS)],
        capture_output=True,
        text=True,
        check=False,
        env=environment,
    )
    if finished.returncode != 0:
        raise AssertionError(
            f"the client harness failed:\n{finished.stdout}\n{finished.stderr}"
        )
    return json.loads(finished.stdout)


class ClientGeometry(unittest.TestCase):
    """Where a click lands on the map."""

    @classmethod
    def setUpClass(cls):
        cls.measured = run_harness()

    def test_a_cursor_position_maps_to_a_cell_at_every_zoom(self):
        # The canvas is sized by its width/height attributes in map cells times the zoom, and the
        # stylesheet gives it no CSS size, so one client pixel is one canvas pixel. At zoom 8 the
        # point (83, 27) is inside cell (10, 3); at zoom 16 the same point is inside cell (5, 1).
        # A mapping that ignored the zoom would answer the same cell twice, and one that divided
        # by the wrong axis would swap them -- 83 and 27 are far enough apart that a transposed
        # read gives (3, 10) rather than anything near the truth.
        self.assertEqual(self.measured["zoom8"], "cell: 10, 3")
        self.assertEqual(self.measured["zoom16"], "cell: 5, 1")

    def test_a_scrolled_page_moves_the_rect_and_not_the_client_coordinates(self):
        # `clientX` is viewport-relative, so scrolling the page moves the element's bounding rect
        # underneath it. With the overlay's rect at (-200, -64) the same (83, 27) is 283 and 91
        # pixels into the canvas, which at zoom 8 is cell (35, 11). A version that used
        # `offsetLeft`, or that forgot the rect, reads (10, 3) here and is off by 25 cells.
        # scrollLeft 214 and a 14-pixel pane padding put the overlay's rect at (-200, -64), so the
        # viewport point (97, 41) is 297 and 105 pixels into the canvas: cell (37, 13) at zoom 8.
        self.assertEqual(self.measured["scrolled"], "cell: 37, 13")
        # The same reading at zoom 16, because zoom-to-cursor depends on both being right and a
        # scrolled page at a non-default zoom is the case the two features share.
        self.assertEqual(self.measured["scrolledZoom16"], "cell: 18, 6")

    def test_a_cursor_outside_the_map_is_clamped_to_it(self):
        self.assertEqual(self.measured["zoom8Origin"], "cell: 0, 0")
        self.assertEqual(self.measured["zoom8LastCell"], "cell: 63, 63")
        # Past the far edge and before the near one: both clamp rather than asking the server to
        # paint a rectangle outside the map.
        self.assertEqual(self.measured["zoom8PastTheEnd"], "cell: 63, 63")
        self.assertEqual(self.measured["zoom8Negative"], "cell: 0, 0")

    def test_ctrl_scrolling_zooms_about_the_cursor_and_not_the_origin(self):
        # At zoom 32 a 128x128 map is 4096 pixels square, so zooming about the top-left throws
        # whatever the user was looking at off the screen. The cell under the cursor must be the
        # same cell after the zoom as before it.
        self.assertEqual(self.measured["beforeZoom"], "cell: 12, 10")
        self.assertEqual(self.measured["zoomAfterWheel"], "9")
        self.assertEqual(self.measured["afterZoom"], self.measured["beforeZoom"])
        # And it got there by scrolling the pane, not by luck: the cursor sat 12.5 cells into the
        # canvas, so growing each cell by one pixel moves that point 12.5 pixels right.
        self.assertEqual(self.measured["scrollAfterWheel"], {"left": 12.5, "top": 10})

    def test_a_plain_wheel_is_left_for_the_pane_to_scroll(self):
        # Hijacking an unmodified wheel makes a map larger than the window impossible to move
        # around. Only ctrl-scroll and a trackpad pinch -- which arrives with ctrlKey set -- zoom.
        self.assertFalse(self.measured["plainWheelChangedZoom"])

    def test_the_selection_rectangle_is_the_same_whichever_corner_the_drag_started_from(self):
        # A drag from (5, 2) to (8, 4) at zoom 8: the outline is inset one pixel on each side, so
        # 4 cells wide is 4 * 8 - 2 = 30 and 3 cells tall is 3 * 8 - 2 = 22.
        self.assertEqual(self.measured["selection"], [[41, 17, 30, 22]])
        # Dragging the other way must give the identical rectangle. Without the normalisation it
        # comes out with negative width and draws nothing visible.
        self.assertEqual(self.measured["selectionBackwards"], [[41, 17, 30, 22]])


class ClientState(unittest.TestCase):
    """What the client does when the server says no."""

    @classmethod
    def setUpClass(cls):
        cls.measured = run_harness()

    def test_opening_a_map_is_a_post_and_never_a_get(self):
        # A state-mutating GET is reachable from a bare `<img src>` on any page in the world.
        self.assertEqual(self.measured["openRequest"]["method"], "POST")
        self.assertEqual(self.measured["openRequest"]["url"], "/api/open")
        self.assertIn("path=", self.measured["openRequest"]["body"])

    def test_every_request_after_an_open_carries_the_handle_it_was_given(self):
        self.assertIn("token=handle-0", self.measured["saveBody"])
        self.assertIn("token=handle-0", self.measured["atlasRequest"])

    def test_the_page_seeds_the_maps_directory_and_lists_it_without_being_asked(self):
        # Typing a 180-character absolute path is what the picker exists to remove, and the first
        # attempt at this tool failed on whitespace pasted into one.
        self.assertEqual(
            self.measured["startupRequests"], ["/api/config", "/api/list"]
        )
        self.assertEqual(self.measured["seededDirectory"], "/fixture/maps")
        self.assertEqual(self.measured["pickerOptions"], ["alpha.scn", "big.scn"])

    def test_opening_rejoins_a_name_the_server_itself_listed(self):
        self.assertEqual(
            self.measured["openRequest"]["body"], "path=%2Ffixture%2Fmaps%2Fbig.scn"
        )

    def test_saving_sends_a_filename_and_the_chosen_directory_and_never_a_typed_path(self):
        self.assertEqual(self.measured["saveWhileOpen"], ["/api/save"])
        self.assertEqual(
            self.measured["saveBody"], "dir=%2Ffixture%2Fmaps&name=out.scn&token=handle-0"
        )
        # An empty filename is refused in the page rather than sent for the server to reject.
        self.assertEqual(self.measured["saveWithNoName"], [])

    def test_a_drag_released_outside_the_window_does_not_stay_live(self):
        self.assertTrue(self.measured["dragStarted"], "the drag never started")
        # Without these the selection goes on tracking the cursor with no button held, and the
        # next click paints a rectangle the user never drew.
        self.assertFalse(self.measured["dragSurvivedBlur"])
        self.assertFalse(self.measured["dragSurvivedButtonRelease"])

    def test_a_refused_open_keeps_the_held_map_and_saving_still_works(self):
        # The server goes on holding the previous map, so the tab must not report it closed --
        # that disagreement is how a typo produced a file the user had been told did not exist.
        self.assertEqual(
            self.measured["heldRefusalLog"],
            ["still holding /fixture/big.scn; the new path was refused"],
        )
        self.assertEqual(self.measured["saveAfterHeldRefusal"], ["/api/save"])

    def test_a_refused_open_with_nothing_held_closes_the_tab_and_blocks_saving(self):
        self.assertEqual(self.measured["summaryAfterEmptyRefusal"], "No map open.")
        # And Save As does not reach the network at all, rather than relying on the server to
        # refuse it.
        self.assertEqual(self.measured["saveAfterEmptyRefusal"], [])


class NativeFileDialog(unittest.TestCase):
    """Browse, for the people who should not have to type an absolute path.

    A web page cannot hand the server a real filesystem path -- `webkitdirectory` gives file
    contents with fake relative names and the File System Access API gives an opaque, Chrome-only
    handle. The server runs on the same machine, so the server opens the dialog. What the page does
    with the three answers is what these pin; the dialog itself needs a human and a desktop.
    """

    @classmethod
    def setUpClass(cls):
        cls.measured = run_harness()

    def test_browsing_fills_the_field_and_lists_the_directory(self):
        self.assertEqual(
            self.measured["browseDirRequests"],
            [
                {"url": "/api/pick-directory", "method": "POST"},
                {"url": "/api/list?dir=%2Fchosen%2Fmaps", "method": "GET"},
            ],
        )
        # The chosen path lands in the typed field, so the two are the same thing afterwards and
        # the user can edit what the dialog gave them.
        self.assertEqual(self.measured["browsedDirectoryField"], "/chosen/maps")

    def test_dismissing_the_dialog_changes_nothing_and_says_nothing(self):
        # A cancel is an ordinary act. Turning it into a line in the log -- the one place this
        # editor says things that matter -- trains people to stop reading it.
        self.assertFalse(self.measured["cancelChangedTheField"])
        self.assertFalse(self.measured["cancelLoggedAnything"])
        self.assertFalse(self.measured["cancelRelisted"])

    def test_no_dialog_is_a_refusal_that_leaves_the_typed_field_working(self):
        self.assertEqual(
            self.measured["unavailableLog"],
            ["there is no desktop session. Type the path instead"],
        )
        # And the field it points at still does the job, which is what keeps this an accelerator
        # rather than a replacement -- it has to survive SSH and a headless box.
        self.assertEqual(self.measured["typedStillWorks"], ["/api/list"])

    def test_a_browsed_save_path_is_sent_whole_and_a_typed_one_is_not(self):
        # A browsed path may be in any directory the user navigated to, so it goes as `path`, which
        # the server guards with create-new and the device-and-inode check. A typed name goes as a
        # directory plus one plain component, which the server also confines to that directory.
        self.assertEqual(
            self.measured["saveAfterBrowse"], "path=%2Felsewhere%2Fpicked.scn&token=handle-0"
        )
        # Typing over a browsed path goes back to the confined form: whichever the user touched
        # last is the one that counts, and a stale browsed path must not outlive it.
        self.assertEqual(
            self.measured["saveAfterTyping"],
            "dir=%2Ffixture%2Fmaps&name=typed.scn&token=handle-0",
        )


class BundleLimitationIsOnScreen(unittest.TestCase):
    """The `.app` bundle warning has to be where the user is when it bites.

    On a Wine-wrapper install -- which is how most of this community runs the game -- the maps live
    inside a `.app` bundle, and macOS's folder chooser greys bundles out and will not descend into
    one by clicking. A note in the README is useless to somebody standing in front of the dialog
    right now, so it is on the page, next to the button.
    """

    def setUp(self):
        self.html = (
            ROOT / "spikes" / "asset-viewer" / "src" / "ui" / "index.html"
        ).read_text()

    def test_the_page_names_the_escape_hatch_beside_the_browse_button(self):
        # Cmd+Shift+G is the only way into a bundle from the chooser, and it is worth nothing in a
        # document the user is not reading at that moment.
        self.assertIn("Cmd+Shift+G", self.html)
        # Adjacent, and said as "nothing comes between them" rather than as a character budget: a
        # budget generous enough not to be brittle is generous enough to let a paragraph or two
        # slide in, which is exactly the drift it was meant to catch.
        browse = self.html.index('id="browse-dir"')
        # The hint's own opening tag, not the id inside it -- otherwise the `<p` being looked for
        # below is the hint's own and the check can never fail.
        hint = self.html.index('<p class="hint" id="bundle-hint"')
        self.assertLess(browse, hint, "the warning comes before the button it is about")
        between = self.html[browse:hint]
        self.assertNotIn(
            "<p",
            between,
            "another paragraph has come between the Browse button and its warning",
        )
        self.assertNotIn("<form", between, "the warning has moved out of the open form")
        # The real gap is one closing button and one closing form tag. Held tight on purpose.
        self.assertLess(len(between), 160, f"the warning has drifted: {between!r}")

    def test_the_page_says_which_path_actually_works(self):
        # The reliable route on the install this was found on is the pre-seeded field plus List,
        # not the dialog. Saying so is the difference between a user who gets to a map and one who
        # concludes the tool is broken.
        hint = self.html[self.html.index('id="bundle-hint"'):]
        hint = hint[: hint.index("</p>")]
        self.assertIn("List", hint)
        self.assertIn(".app", hint)
        self.assertIn("Cmd+Shift+G", hint)


class CanvasSizingContract(unittest.TestCase):
    """The stylesheet invariant the pixel mapping rests on.

    `cellAt` subtracts the bounding rect and divides by the zoom, which is only the right answer
    while one canvas pixel is one CSS pixel -- that is, while the stylesheet sets no `width` or
    `height` on the canvases and the client sets the attributes itself. A CSS rule adding either
    would scale the canvas and silently offset every paint, with the harness above still passing
    because it models the rect the same way. This is the check for that.
    """

    # Every way the two canvases can be selected. `#overlay` names one of them without the word
    # "canvas" appearing anywhere in the selector, which is how the first version of this check
    # passed a stylesheet that had just been given `#overlay { width: 100% }`.
    CANVAS_SELECTORS = ("canvas", "#map", "#overlay")

    def test_the_check_below_can_see_every_rule_that_reaches_a_canvas(self):
        # A guard whose matcher is wrong is a guard that is not there. This pins the matcher
        # against the stylesheet as written: each of these rules must be one the size check looks
        # at, and the palette chips must be the only canvases it deliberately skips.
        selectors = [selector for selector, _ in self.rules()]
        for reaching in ("#canvas-wrap canvas", "#canvas-wrap canvas#map", "#overlay"):
            self.assertIn(reaching, selectors, "style.css no longer has this rule")
            self.assertTrue(
                self.reaches_a_map_canvas(reaching),
                f"the size check does not look at `{reaching}`, which styles a map canvas",
            )
        self.assertFalse(self.reaches_a_map_canvas(".swatch canvas"))

    @staticmethod
    def rules():
        css = STYLE_CSS.read_text()
        return [
            (selector.strip(), body)
            for selector, body in re.findall(r"([^{}]*)\{([^}]*)\}", css)
        ]

    @classmethod
    def reaches_a_map_canvas(cls, selector):
        """Whether a rule could size one of the two map canvases.

        The palette chips are canvases too, but they are drawn at a fixed 20x20 and nothing
        measures a cursor against them, so a CSS size there is harmless.
        """
        if ".swatch" in selector:
            return False
        return any(token in selector for token in cls.CANVAS_SELECTORS)

    def test_the_stylesheet_sets_no_css_size_on_either_canvas(self):
        for selector, body in self.rules():
            if not self.reaches_a_map_canvas(selector):
                continue
            for property_name in ("width", "height"):
                self.assertNotRegex(
                    body,
                    rf"(^|[;\s]){property_name}\s*:",
                    f"`{selector}` sets a CSS {property_name} on a map canvas. That scales it "
                    f"away from its attribute size, so every click lands on the wrong cell while "
                    f"every other test still passes.",
                )

    def test_the_client_sizes_the_canvases_from_their_attributes(self):
        app = APP_JS.read_text()
        for assignment in (
            "mapCanvas.width = state.width * z;",
            "mapCanvas.height = state.height * z;",
            "overlay.width = mapCanvas.width;",
            "overlay.height = mapCanvas.height;",
        ):
            self.assertIn(assignment, app)


if __name__ == "__main__":
    unittest.main()


class PlatformHints(unittest.TestCase):
    """The "type a path instead" instruction, which is a different key on every platform.

    This text said ``Cmd+Shift+G`` unconditionally while macOS was the only platform with a
    dialog. Now that Browse opens a GTK, Qt or Windows chooser too, an unqualified macOS keystroke
    is an instruction that does nothing for most of the people reading it -- and the page is the
    surface that matters, because a note in a README is worth nothing to somebody standing in
    front of the dialog right now.
    """

    def test_each_platform_is_told_its_own_way_to_type_a_path(self):
        expected = {
            "MacIntel": ("Cmd+Shift+G", False),
            "Win32": ("File name box", True),
            "Linux x86_64": ("Ctrl+L", True),
        }
        for platform, (key, bundle_note_hidden) in expected.items():
            with self.subTest(platform=platform):
                hints = run_harness(platform)["platformHints"]
                self.assertIn(key, hints["open"])
                self.assertIn(key, hints["save"])
                # The `.app` bundle warning is macOS's rule. Left up elsewhere it sends a Linux
                # user looking for a problem their chooser does not have.
                self.assertEqual(hints["bundleNoteHidden"], bundle_note_hidden)

    def test_no_platform_is_offered_another_platforms_shortcut(self):
        for platform, wrong in [
            ("Win32", "Cmd+Shift+G"),
            ("Linux x86_64", "Cmd+Shift+G"),
            ("MacIntel", "Ctrl+L"),
        ]:
            with self.subTest(platform=platform):
                hints = run_harness(platform)["platformHints"]
                self.assertNotIn(wrong, hints["open"] + hints["save"])
