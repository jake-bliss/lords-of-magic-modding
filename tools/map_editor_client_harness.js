// Run the map editor's real client against a stub DOM and report what it computed.
//
// The client is 300 lines of pixel-to-cell arithmetic that no Rust test can see: every handler
// test drives `Editor::handle` directly, so the browser half -- which decides *where* a paint
// lands -- was covered by reading the code and nothing else. This loads `src/ui/app.js`
// **verbatim**, so it cannot drift from what the binary serves, drives it through its own event
// handlers, and prints the measurements as JSON for `tests/test_map_editor_client.py` to assert.
//
//   node tools/map_editor_client_harness.js path/to/app.js
//
// It asserts nothing itself. The expected values live in the Python test, where they are readable
// next to the reasoning for them.

"use strict";

const fs = require("fs");
const path = require("path");
const vm = require("vm");

const source = fs.readFileSync(process.argv[2] ?? path.join(
  __dirname, "..", "spikes", "asset-viewer", "src", "ui", "app.js",
), "utf8");

// ---------------------------------------------------------------- the stub DOM
// Only what `app.js` actually touches. If it grows a new element id this throws by name rather
// than silently producing a half-wired page, which is the failure mode that would make the test
// meaningless.

function makeContext() {
  const calls = [];
  const record = (name) => (...args) => calls.push({ name, args });
  return {
    calls,
    imageSmoothingEnabled: false,
    strokeStyle: "",
    lineWidth: 0,
    drawImage: record("drawImage"),
    clearRect: record("clearRect"),
    strokeRect: record("strokeRect"),
  };
}

function makeElement(id) {
  const listeners = new Map();
  const context = makeContext();
  const element = {
    id,
    width: 0,
    height: 0,
    value: "",
    disabled: false,
    textContent: "",
    title: "",
    className: "",
    children: [],
    // Canvases are laid out by their width/height attributes: the stylesheet sets no CSS size, so
    // the rect is the attribute size. The harness overrides this per test to model scrolling.
    rect: { left: 0, top: 0 },
    classList: {
      classes: new Set(),
      add(name) { this.classes.add(name); },
      remove(name) { this.classes.delete(name); },
    },
    style: {},
    listeners,
    addEventListener(type, handler) {
      if (!listeners.has(type)) {
        listeners.set(type, []);
      }
      listeners.get(type).push(handler);
    },
    appendChild(child) { element.children.push(child); },
    querySelectorAll() { return []; },
    getContext() { return context; },
    scrollLeft: 0,
    scrollTop: 0,
    getBoundingClientRect() {
      const rect = typeof element.rect === "function" ? element.rect() : element.rect;
      return { left: rect.left, top: rect.top, width: element.width, height: element.height };
    },
    context,
    async fire(type, event) {
      for (const handler of listeners.get(type) ?? []) {
        await handler({ preventDefault() {}, ...event });
      }
    },
  };
  return element;
}

const IDS = [
  "dir-form", "maps-dir", "map-file", "open-form", "map-summary", "palette", "seed", "undo",
  "zoom", "save-form", "save-name", "map", "overlay", "centre", "cursor-cell", "log",
];
const elements = new Map(IDS.map((id) => [id, makeElement(id)]));

// The zoom slider's bounds come from `index.html` rather than being repeated here, so a change to
// the range cannot leave the harness testing a slider the page does not have.
{
  const html = fs.readFileSync(
    path.join(__dirname, "..", "spikes", "asset-viewer", "src", "ui", "index.html"),
    "utf8",
  );
  const tag = html.match(/<input id="zoom"[^>]*>/);
  if (!tag) {
    throw new Error("index.html has no zoom input for the harness to read bounds from");
  }
  const attribute = (name) => {
    const found = tag[0].match(new RegExp(`${name}="([^"]*)"`));
    if (!found) {
      throw new Error(`the zoom input has no ${name}`);
    }
    return found[1];
  };
  const slider = elements.get("zoom");
  slider.min = attribute("min");
  slider.max = attribute("max");
  slider.value = attribute("value");
}

const windowListeners = new Map();

// The open response the stubbed `fetch` answers with: a 64x64 map through a small fixture tileset,
// large enough that the clamp in `cellAt` does not hide a coordinate the test is measuring.
const LIST = {
  ok: true,
  dir: "/fixture/maps",
  entries: [
    { name: "alpha.scn", size: 100 },
    { name: "big.scn", size: 200 },
  ],
  notes: [],
};

const CONFIG = { ok: true, mapsDirectory: "/fixture/maps", undoDepth: 32, notes: [] };

const OPEN = {
  ok: true,
  path: "/fixture/maps/big.scn",
  token: "handle-0",
  width: 64,
  height: 64,
  class: "world map",
  tileset: {
    member: "fixture.til", provenance: "supplied", atlas: "fixture.lbm",
    columns: 12, rows: 3, tileWidth: 8, tileHeight: 8,
  },
  terrains: [
    { id: 1, description: "grass", tiles: 9, swatch: 0, paintable: true },
    { id: 42, description: "deep cave", tiles: 3, swatch: 33, paintable: true },
  ],
  tiles: new Array(64 * 64).fill(0),
  notes: [],
};

const requests = [];
const sandbox = {
  console,
  URLSearchParams,
  Date,
  Math,
  Number,
  String,
  Array,
  JSON,
  Error,
  Promise,
  document: {
    getElementById(id) {
      const element = elements.get(id);
      if (!element) {
        throw new Error(`app.js asked for an element this harness does not stub: ${id}`);
      }
      return element;
    },
    createElement: (tag) => makeElement(`created:${tag}`),
  },
  window: {
    addEventListener(type, handler) {
      if (!windowListeners.has(type)) {
        windowListeners.set(type, []);
      }
      windowListeners.get(type).push(handler);
    },
  },
  Image: class {
    set src(value) {
      this._src = value;
      sandbox.__atlasSrc = value;
      queueMicrotask(() => this.onload && this.onload());
    }
    get src() { return this._src; }
  },
  async fetch(url, options) {
    const body = options?.body ?? "";
    requests.push({ url, method: options?.method ?? "GET", body });
    let payload = { ok: true, notes: [], undoDepth: 1 };
    if (url.startsWith("/api/config")) {
      payload = CONFIG;
    } else if (url.startsWith("/api/list")) {
      payload = url.includes("empty") ? { ...LIST, entries: [] } : LIST;
    } else if (url.startsWith("/api/open")) {
      // Two refusal shapes, because they must behave differently: one while the server still holds
      // a map, and one with nothing open at all.
      if (body.includes("held")) {
        payload = { ok: false, refusal: "no such file", holding: "/fixture/big.scn", notes: [] };
      } else if (body.includes("nothing")) {
        payload = { ok: false, refusal: "no such file", holding: null, notes: [] };
      } else {
        payload = OPEN;
      }
    }
    return { status: 200, text: async () => JSON.stringify(payload) };
  },
};
sandbox.globalThis = sandbox;
vm.createContext(sandbox);
vm.runInContext(source, sandbox, { filename: "app.js" });

/// Let every pending promise in the client settle.
///
/// `openMap` is started by a non-async submit handler, so firing the event only *begins* it; the
/// fetch and the atlas load each take a turn. Without this the measurements read a client that has
/// not finished opening, which looks exactly like a broken pixel mapping.
function settle() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

async function fireWindow(type, event) {
  for (const handler of windowListeners.get(type) ?? []) {
    await handler({ preventDefault() {}, ...event });
  }
}

const overlay = elements.get("overlay");
const readout = elements.get("cursor-cell");
const zoom = elements.get("zoom");

/// Read the cell the client thinks the cursor is over.
async function cellAt(clientX, clientY) {
  await overlay.fire("mousemove", { clientX, clientY, buttons: 0 });
  return readout.textContent;
}

(async () => {
  const measurements = {};

  // The page asks what it can fill in for itself, and lists that directory without being told.
  await settle();
  await settle();
  measurements.startupRequests = requests.map((request) => request.url.split("?")[0]);
  measurements.seededDirectory = elements.get("maps-dir").value;
  measurements.pickerOptions = elements.get("map-file").children.map((option) => option.value);

  // Open the map the picker offers, through the real submit handler.
  requests.length = 0;
  elements.get("map-file").value = "big.scn";
  await elements.get("open-form").fire("submit", {});
  await settle();
  await settle();
  measurements.openRequest = requests.find((request) => request.url === "/api/open");
  measurements.atlasRequest = sandbox.__atlasSrc;

  // The overlay lives in a scrolling pane with 14 pixels of padding, so its rect moves with the
  // scroll exactly as it does in the browser.
  const centre = elements.get("centre");
  overlay.rect = () => ({ left: 14 - centre.scrollLeft, top: 14 - centre.scrollTop });
  const sizeTo = (z) => {
    zoom.value = String(z);
    overlay.width = 64 * z;
    overlay.height = 64 * z;
  };

  sizeTo(8);
  centre.scrollLeft = 0;
  centre.scrollTop = 0;
  measurements.zoom8 = await cellAt(14 + 83, 14 + 27);
  measurements.zoom8Origin = await cellAt(14, 14);
  measurements.zoom8LastCell = await cellAt(14 + 511, 14 + 511);
  measurements.zoom8PastTheEnd = await cellAt(9000, 9000);
  measurements.zoom8Negative = await cellAt(-40, -40);

  sizeTo(16);
  measurements.zoom16 = await cellAt(14 + 83, 14 + 27);

  // A scrolled pane: the rect moves, the client coordinates do not.
  sizeTo(8);
  centre.scrollLeft = 214;
  centre.scrollTop = 78;
  measurements.scrolled = await cellAt(14 + 83, 14 + 27);
  // The same reading at a different zoom, because zoom-to-cursor depends on both being right.
  sizeTo(16);
  measurements.scrolledZoom16 = await cellAt(14 + 83, 14 + 27);

  // Ctrl-scroll zooms about the cursor: the cell under it must not move.
  sizeTo(8);
  centre.scrollLeft = 0;
  centre.scrollTop = 0;
  const cursor = { clientX: 114, clientY: 94 };
  measurements.beforeZoom = await cellAt(cursor.clientX, cursor.clientY);
  await overlay.fire("wheel", { ...cursor, ctrlKey: true, deltaY: -1 });
  measurements.zoomAfterWheel = zoom.value;
  measurements.scrollAfterWheel = { left: centre.scrollLeft, top: centre.scrollTop };
  overlay.width = 64 * Number(zoom.value);
  overlay.height = 64 * Number(zoom.value);
  measurements.afterZoom = await cellAt(cursor.clientX, cursor.clientY);

  // A plain wheel is left for the pane to scroll.
  const zoomBeforePlainWheel = zoom.value;
  await overlay.fire("wheel", { ...cursor, ctrlKey: false, deltaY: -1 });
  measurements.plainWheelChangedZoom = zoom.value !== zoomBeforePlainWheel;

  // A drag released outside the window must not stay live.
  sizeTo(8);
  centre.scrollLeft = 0;
  centre.scrollTop = 0;
  overlay.context.calls.length = 0;
  await overlay.fire("mousedown", { clientX: 40, clientY: 40, buttons: 1 });
  measurements.dragStarted = overlay.context.calls.some((call) => call.name === "strokeRect");
  await fireWindow("blur", {});
  overlay.context.calls.length = 0;
  await overlay.fire("mousemove", { clientX: 200, clientY: 200, buttons: 0 });
  measurements.dragSurvivedBlur = overlay.context.calls.some((call) => call.name === "strokeRect");

  await overlay.fire("mousedown", { clientX: 40, clientY: 40, buttons: 1 });
  overlay.context.calls.length = 0;
  await overlay.fire("mousemove", { clientX: 200, clientY: 200, buttons: 0 });
  measurements.dragSurvivedButtonRelease =
    overlay.context.calls.some((call) => call.name === "strokeRect");

  // The selection rectangle for a drag from (5, 2) to (8, 4) at zoom 8.
  await overlay.fire("mousedown", { clientX: 14 + 5 * 8 + 3, clientY: 14 + 2 * 8 + 3, buttons: 1 });
  overlay.context.calls.length = 0;
  await overlay.fire("mousemove", { clientX: 14 + 8 * 8 + 3, clientY: 14 + 4 * 8 + 3, buttons: 1 });
  measurements.selection = overlay.context.calls
    .filter((call) => call.name === "strokeRect")
    .map((call) => call.args);

  await overlay.fire("mousedown", { clientX: 14 + 8 * 8 + 3, clientY: 14 + 4 * 8 + 3, buttons: 1 });
  overlay.context.calls.length = 0;
  await overlay.fire("mousemove", { clientX: 14 + 5 * 8 + 3, clientY: 14 + 2 * 8 + 3, buttons: 1 });
  measurements.selectionBackwards = overlay.context.calls
    .filter((call) => call.name === "strokeRect")
    .map((call) => call.args);
  await fireWindow("blur", {});

  // Save is a filename inside the chosen directory, never a typed absolute path.
  requests.length = 0;
  elements.get("save-name").value = "out.scn";
  await elements.get("save-form").fire("submit", {});
  await settle();
  measurements.saveWhileOpen = requests.map((request) => request.url);
  measurements.saveBody = requests[0].body;

  // An empty filename does not reach the network.
  requests.length = 0;
  elements.get("save-name").value = "   ";
  await elements.get("save-form").fire("submit", {});
  await settle();
  measurements.saveWithNoName = requests.map((request) => request.url);
  elements.get("save-name").value = "out.scn";

  // A refused open while the server still holds a map: the tab stays open on the held map, says
  // which, and can still save it.
  requests.length = 0;
  elements.get("map-file").value = "held.scn";
  await elements.get("open-form").fire("submit", {});
  await settle();
  await settle();
  measurements.heldRefusalLog = elements.get("log").children.map((row) => row.textContent)
    .filter((text) => text.includes("still holding"));
  requests.length = 0;
  await elements.get("save-form").fire("submit", {});
  await settle();
  measurements.saveAfterHeldRefusal = requests.map((request) => request.url);

  // A refused open with nothing held: the tab closes and Save As does not reach the network.
  elements.get("map-file").value = "nothing.scn";
  await elements.get("open-form").fire("submit", {});
  await settle();
  await settle();
  measurements.summaryAfterEmptyRefusal = elements.get("map-summary").textContent;
  requests.length = 0;
  await elements.get("save-form").fire("submit", {});
  await settle();
  measurements.saveAfterEmptyRefusal = requests.map((request) => request.url);

  console.log(JSON.stringify(measurements, null, 2));
})().catch((error) => {
  console.error(String(error && error.stack ? error.stack : error));
  process.exit(1);
});
