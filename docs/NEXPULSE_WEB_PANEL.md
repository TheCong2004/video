# NexPulse in Concat

Windows launcher sidebar -> NexPulse. This is an actual child WebView2 inside
the native window, not an iframe, screenshot, or external-browser shortcut.
The page occupies the content viewport; the existing editor stays native.
This is the user's explicit exception to the otherwise-native Studio surface.

URL: https://deloy-internet-frontend.vercel.app/launches

The web engine starts lazily when the section is selected. The toolbar has Home
and Reload/Retry. The same view follows window bounds/DPI, hides when another
route/editor/dialog/menu is shown or the parent is minimized, and is reused on
return. A 30-second loading timeout gives a retry action, not a claimed success.
Missing WebView2 Runtime gives an actionable error; it is not installed silently.

## Boundary and current limitations

- No native IPC, injected script, custom protocol, project file API or Concat
  credentials are exposed to the remote page. It cannot edit a local timeline.
- Only the supplied HTTPS origin is allowed for top-level navigation.
- Popups/new windows and downloads are blocked; remote camera, microphone,
  location and other permission requests are denied. Cross-origin social OAuth
  popup sign-in is not implemented by this embedding and must not be advertised
  as tested. Viewing the site does not authorize Concat to publish any post.
- NexPulse's own page/network/backend behavior is unchanged. Document load does
  not mean its backend, authentication or scheduling features have been tested.
- The profile is separate from Chrome/Edge and Concat AI keys, under
  LOCALAPPDATA/app.concat.editor/web/nexpulse. No browser cookies are imported.
  Only this webview/browser engine owns its own session data. Public navigation
  does not sign the user into anything automatically.
- The implementation currently supports Windows; other platforms show a clear
  unsupported-platform message rather than opening an external browser.

## Verification

`node --test scripts/web-embed-regression.test.mjs` covers sidebar/route/boundary
wiring. `tools/webview-preview` imports the real Slint web panel and native
controller, without linking media/AI engines. Its opt-in `--smoke` loads the
public page without login or posting and checks the real document title, child
bounds, route hide, resizing, modal hide and minimize/restore. Its Rust unit test
rejects hostile hosts, HTTP, local/file/javascript URLs, userinfo and ports.

Use an isolated LOCALAPPDATA for the smoke process; never the user's profile.
Build intermediates belong under the task's temporary target on D and should be
removed after the final EXE is verified. Keep only the latest runnable Concat
and required runtime DLLs; do not invoke a second full build for screenshots.
