// SpiderDesk windows: tells the SpiderDesk desktop pet where the application windows are.
//
// On Wayland an ordinary program cannot list other programs' windows, so the pets would have
// nothing real to walk on. This extension runs inside GNOME Shell and, only while SpiderDesk is
// running, writes $XDG_RUNTIME_DIR/spiderdesk/windows.txt about ten times per second:
//
//   pointer X Y
//   elem N X Y WIDTH HEIGHT name                  (Shell elements: the top bar)
//   win ID X Y WIDTH HEIGHT PID MAXIMIZED title up to the end of the line
//
// Items are listed front to back. SpiderDesk signals that it is running by rewriting the file
// `alive` in the same directory; when that file is older than a few seconds the extension only
// checks it again every two seconds, writes nothing and deletes what it wrote.
//
// Window pictures. By default only geometry, PID and title leave the Shell. When SpiderDesk's
// player character is on screen it asks for more by writing `capture=1` into `alive`: then the
// three frontmost windows are also saved as `cap-<ID>.png` (about once per second each), so the
// game can find the visible lines inside the applications (text lines, buttons, boxes) and let
// the character walk on them. The pictures stay in the user-private runtime directory (mode
// 0700, in memory), are overwritten in place and are deleted as soon as SpiderDesk stops asking.

import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const ACTIVE_MS = 100;
const IDLE_MS = 2000;
const ALIVE_MAX_AGE_SECS = 5;
// rewrite an unchanged file this often so SpiderDesk can tell the extension is still there
const KEEPALIVE_SECS = 1;
// window pictures: one window every CAPTURE_MS, cycling through the frontmost CAPTURE_WINDOWS
const CAPTURE_MS = 400;
const CAPTURE_WINDOWS = 3;

export default class SpiderDeskWindows extends Extension {
    enable() {
        const dir = GLib.build_filenamev([GLib.get_user_runtime_dir(), 'spiderdesk']);
        this._dir = dir;
        this._out = GLib.build_filenamev([dir, 'windows.txt']);
        this._request = {};
        this._captured = new Set();
        this._capturing = false;
        this._lastCapture = 0;
        this._captureTurn = 0;
        this._alive = Gio.File.new_for_path(GLib.build_filenamev([dir, 'alive']));
        this._last = '';
        this._lastWrite = 0;
        this._timer = 0;
        this._schedule(IDLE_MS);
    }

    disable() {
        if (this._timer) {
            GLib.source_remove(this._timer);
            this._timer = 0;
        }
        this._dropCaptures();
        this._alive = null;
        this._last = '';
    }

    _schedule(ms) {
        this._timer = GLib.timeout_add(GLib.PRIORITY_LOW, ms, () => {
            let next = IDLE_MS;
            try {
                if (this._appAlive()) {
                    this._publish();
                    next = ACTIVE_MS;
                } else {
                    this._dropCaptures();
                }
            } catch (e) {
                console.error(`SpiderDesk windows: ${e}`);
            }
            this._schedule(next);
            return GLib.SOURCE_REMOVE;
        });
    }

    _appAlive() {
        try {
            const info = this._alive.query_info(
                'time::modified', Gio.FileQueryInfoFlags.NONE, null);
            const modified = info.get_attribute_uint64('time::modified');
            if (GLib.get_real_time() / 1e6 - modified >= ALIVE_MAX_AGE_SECS)
                return false;
            // "pid=1234 capture=1"
            const [, bytes] = this._alive.load_contents(null);
            this._request = {};
            for (const part of new TextDecoder().decode(bytes).split(/\s+/)) {
                const [key, value] = part.split('=');
                if (key && value !== undefined)
                    this._request[key] = value;
            }
            return true;
        } catch {
            return false;
        }
    }

    // Saves the picture of one window (its own content, without whatever is drawn over it).
    _captureNext(windows) {
        if (this._request.capture !== '1') {
            this._dropCaptures();
            return;
        }
        const now = GLib.get_monotonic_time() / 1000;
        if (this._capturing || windows.length === 0 || now - this._lastCapture < CAPTURE_MS)
            return;
        this._lastCapture = now;
        // pictures of windows that are no longer among the frontmost ones are stale
        const keep = new Set(windows.map(w => w.id));
        for (const id of [...this._captured]) {
            if (!keep.has(id))
                this._deleteCapture(id);
        }
        const {id, actor, window} = windows[this._captureTurn++ % windows.length];
        const content = actor.paint_to_content(null);
        const texture = content?.get_texture();
        if (!texture)
            return;
        // the texture covers the buffer (with shadows); keep only the visible frame
        const frame = window.get_frame_rect();
        const buffer = window.get_buffer_rect();
        const scale = texture.get_width() / Math.max(1, buffer.width);
        const x = Math.max(0, Math.round((frame.x - buffer.x) * scale));
        const y = Math.max(0, Math.round((frame.y - buffer.y) * scale));
        const width = Math.min(texture.get_width() - x, Math.round(frame.width * scale));
        const height = Math.min(texture.get_height() - y, Math.round(frame.height * scale));
        if (width < 16 || height < 16)
            return;
        const tmp = Gio.File.new_for_path(GLib.build_filenamev([this._dir, `cap-${id}.tmp`]));
        const final = Gio.File.new_for_path(GLib.build_filenamev([this._dir, `cap-${id}.png`]));
        const stream = tmp.replace(null, false, Gio.FileCreateFlags.PRIVATE, null);
        this._capturing = true;
        Shell.Screenshot.composite_to_stream(
            texture, x, y, width, height, 1, null, 0, 0, 1, stream, (_source, result) => {
                this._capturing = false;
                try {
                    Shell.Screenshot.composite_to_stream_finish(result);
                    stream.close(null);
                    if (this._alive && this._request.capture === '1') {
                        tmp.move(final, Gio.FileCopyFlags.OVERWRITE, null, null);
                        this._captured.add(id);
                    } else {
                        tmp.delete(null);
                    }
                } catch (e) {
                    console.error(`SpiderDesk windows: capture failed: ${e}`);
                }
            });
    }

    _deleteCapture(id) {
        this._captured.delete(id);
        for (const ext of ['png', 'tmp']) {
            try {
                Gio.File.new_for_path(
                    GLib.build_filenamev([this._dir, `cap-${id}.${ext}`])).delete(null);
            } catch {
                // already gone
            }
        }
    }

    _dropCaptures() {
        for (const id of [...(this._captured ?? [])])
            this._deleteCapture(id);
    }

    _publish() {
        const [px, py] = global.get_pointer();
        const lines = [`pointer ${Math.round(px)} ${Math.round(py)}`];
        // the top bar is something to walk on too (hidden while a window is fullscreen)
        const bar = Main.layoutManager?.panelBox;
        if (bar && bar.visible && bar.height > 1 && !Main.overview.visible) {
            const [bx, by] = bar.get_transformed_position();
            lines.push(
                `elem 1 ${Math.round(bx)} ${Math.round(by)} ` +
                `${Math.round(bar.width)} ${Math.round(bar.height)} top-bar`);
        }
        const workspace = global.workspace_manager.get_active_workspace();
        // in the overview the windows are drawn as thumbnails elsewhere: publish none
        const actors = Main.overview.visible ? [] : global.get_window_actors(); // bottom to top
        const pictured = [];
        for (let i = actors.length - 1; i >= 0; i--) {
            const w = actors[i].meta_window;
            if (!w || w.minimized || !w.located_on_workspace(workspace))
                continue;
            const type = w.get_window_type();
            if (type !== Meta.WindowType.NORMAL &&
                type !== Meta.WindowType.DIALOG &&
                type !== Meta.WindowType.UTILITY)
                continue;
            const r = w.get_frame_rect();
            const maximized =
                (typeof w.is_maximized === 'function' && w.is_maximized()) ||
                (w.maximized_horizontally && w.maximized_vertically);
            const id = typeof w.get_id === 'function' ? w.get_id() : w.get_stable_sequence();
            const title = (w.get_title() ?? '').replace(/[\r\n]+/g, ' ');
            lines.push(
                `win ${id} ${r.x} ${r.y} ${r.width} ${r.height} ${w.get_pid()} ` +
                `${maximized ? 1 : 0} ${title}`);
            if (pictured.length < CAPTURE_WINDOWS &&
                `${w.get_pid()}` !== this._request.pid && r.width >= 120 && r.height >= 80)
                pictured.push({id, actor: actors[i], window: w});
        }
        this._captureNext(pictured);
        const text = `${lines.join('\n')}\n`;
        const now = GLib.get_monotonic_time() / 1e6;
        if (text === this._last && now - this._lastWrite < KEEPALIVE_SECS)
            return;
        GLib.file_set_contents(this._out, text);
        this._last = text;
        this._lastWrite = now;
    }
}
