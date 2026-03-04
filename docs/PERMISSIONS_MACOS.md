# macOS Permissions for Operator Jack

## Why Accessibility Permission Is Needed

Operator Jack uses the macOS Accessibility API (`AXUIElement`) to interact with application interfaces -- finding buttons, reading text, clicking elements, and typing. This API is gated behind the **Accessibility** permission in macOS Privacy & Security settings.

Without this permission, all `ui.*` step types will fail because the system blocks access to other applications' UI element trees.

System-level steps (`sys.*`) that only perform file operations, process management, and URL opening do **not** require Accessibility permission. You can use operator-jack for file automation without granting this permission.

## Checking Permission Status

Run the built-in doctor command to check whether Accessibility permission is granted:

```
operator-jack doctor
```

This command asks the macOS helper to call `AXIsProcessTrusted()` and reports the result. You will see one of:

```
Accessibility: granted
```

or

```
Accessibility: NOT granted
  The terminal application needs Accessibility permission.
  Open System Settings > Privacy & Security > Accessibility and add your terminal app.
```

## Which Application Needs Permission

A common source of confusion: the **terminal application** you are using needs the permission, not the `operator-jack` binary or the `macos-helper` binary.

This is because macOS grants Accessibility permission to the process at the top of the process tree that owns the window. When you run `operator-jack` from Terminal.app, it is Terminal.app that macOS checks for permission.

### Common Terminal Applications

| Terminal App | Bundle Identifier |
|---|---|
| Terminal.app | `com.apple.Terminal` |
| iTerm2 | `com.googlecode.iterm2` |
| Alacritty | `org.alacritty` |
| Warp | `dev.warp.Warp-Stable` |
| Kitty | `net.kovidgoyal.kitty` |
| WezTerm | `org.wezfurlong.wezterm` |
| VS Code integrated terminal | `com.microsoft.VSCode` |

Grant permission to whichever terminal you use to run operator-jack.

## Step-by-Step: Granting Accessibility Permission

### macOS Ventura (13) and Later

1. Open **System Settings** (Apple menu > System Settings).
2. Click **Privacy & Security** in the sidebar.
3. Scroll down and click **Accessibility**.
4. Click the lock icon at the bottom left and authenticate if prompted.
5. Click the **+** button.
6. Navigate to your terminal application:
   - For Terminal.app: `/System/Applications/Utilities/Terminal.app`
   - For iTerm2: `/Applications/iTerm.app`
   - For Alacritty: `/Applications/Alacritty.app`
   - For VS Code: `/Applications/Visual Studio Code.app`
7. Select the application and click **Open**.
8. Ensure the toggle next to the application name is **on**.
9. **Quit and restart your terminal application.** The permission takes effect only after a full restart of the terminal process.

### macOS Monterey (12) and Earlier

1. Open **System Preferences** (Apple menu > System Preferences).
2. Click **Security & Privacy**.
3. Click the **Privacy** tab.
4. Select **Accessibility** from the left sidebar.
5. Click the lock icon at the bottom left and authenticate.
6. Click the **+** button and add your terminal application.
7. Ensure the checkbox next to the application name is checked.
8. **Quit and restart your terminal application.**

## Post-Grant Verification

After granting permission and restarting your terminal, verify by running:

```
operator-jack doctor
```

You should see:

```
Accessibility: granted
```

If it still shows "NOT granted", see the Troubleshooting section below.

## Troubleshooting

### Permission Shows Granted but UI Steps Still Fail

- **Restart the terminal.** The permission is cached at process launch. You must fully quit (not just close the window) and reopen the terminal.
- **Check you granted the right app.** If you use VS Code's integrated terminal, grant permission to VS Code, not Terminal.app.

### Permission Was Revoked After macOS Update

macOS sometimes resets Accessibility permissions after a major OS update. If `operator-jack doctor` reports "NOT granted" after updating macOS:

1. Open System Settings > Privacy & Security > Accessibility.
2. Remove your terminal app from the list (select it, click the **-** button).
3. Re-add it using the **+** button.
4. Restart your terminal.

### Multiple Terminal Apps Listed

If you have multiple terminal apps in the Accessibility list, ensure the one you are actually using has its toggle enabled. Having Terminal.app enabled does not help if you are running operator-jack from iTerm2.

### "operator-jack doctor" Says "Helper Not Found"

This means the macOS helper binary is not installed or not in the expected location. The helper is built separately from the Rust CLI:

```
cd macos-helper
swift build -c release
```

The built binary must be accessible to operator-jack at runtime. See the project README for setup instructions.

### Running from SSH or a Remote Session

Accessibility permission requires a window server connection. If you are running operator-jack over SSH, there is no GUI session and Accessibility API calls will fail regardless of permission settings. UI automation steps require a local GUI session.

### Automation Permission (Separate from Accessibility)

For some UI interactions, macOS may also prompt for **Automation** permission (System Settings > Privacy & Security > Automation). This permission allows one app to control another via Apple Events. If you see a system dialog asking to allow your terminal to control another app, click **OK**. This is a separate permission from Accessibility and may be required for certain `ui.*` operations.
