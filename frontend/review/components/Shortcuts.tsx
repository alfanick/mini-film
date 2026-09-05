/** Render keyboard help as an accessible state-controlled dialog so shortcuts stay discoverable. */
import type { ComponentChildren } from "preact";
import { Dialog } from "./Dialog";

/** Keep the established shortcut descriptions beside their declarative help UI. */
export function ShortcutsOverlay({ open, onClose }: { open: boolean; onClose: () => void }): ComponentChildren {
  const sections: [string, [string[], string][]][] = [
    [
      "Pictures",
      [
        [["←", "→"], "Previous or next picture without changing the rating."],
        [["Enter"], "Move to the next picture without changing the rating."],
      ],
    ],
    ["Histogram", [[["h"], "Show or hide the luma and RGB histogram."]]],
    ["Information", [[["i"], "Show or hide camera focus points on the picture."]]],
    [
      "Keyboard controls",
      [
        [["Tab"], "Focus buttons, fields, the photo viewer, and crop controls."],
        [["Enter", "Space"], "Activate the focused control; toggle full zoom when the photo has focus."],
        [["Esc"], "Close a dialog or full zoom. Dialogs keep focus inside and return it to the opening control."],
        [
          ["Crop arrows"],
          "Move the focused crop frame or resize its corner by one displayed pixel; hold Shift for ten.",
        ],
      ],
    ],
    [
      "Touch / Mouse",
      [
        [["Swipe ←/→"], "Move between visible pictures without changing the rating."],
        [["Swipe ↑/↓"], "Change the rating without advancing."],
        [["Wheel ←/→"], "Move between visible pictures after a short scroll threshold."],
        [["Wheel ↑/↓"], "Preview the previous or next profile after a short scroll threshold."],
        [["Hold"], "Show a nearby loupe for the picture under the cursor or finger until released."],
        [["Double-click"], "Toggle full-image zoom; move the cursor to pan the zoomed image."],
        [
          ["Profile"],
          "Click a profile thumbnail to preview it; use its checkbox to make it available for this picture. " +
            "Double-click or double-tap a profile to make only that profile available.",
        ],
      ],
    ],
    [
      "Rating",
      [
        [["`", "§", "1", "2", "3", "4", "5"], "Set rating and advance to the next visible picture."],
        [["↑", "↓"], "Increase or decrease the rating, then advance."],
      ],
    ],
    [
      "Labels",
      [
        [["6", "7", "8", "9", "0"], "Toggle red, yellow, green, blue, or purple labels without advancing."],
        [["r", "y", "g", "b", "p"], "Same label toggles using mnemonic keys."],
        [["n"], "Clear all color labels."],
      ],
    ],
    [
      "Adjustments",
      [
        [["c"], "Copy the current retouch slider adjustments."],
        [["v"], "Paste copied retouch slider adjustments to the current picture."],
      ],
    ],
    [
      "Profiles",
      [
        [["PgUp", "PgDn"], "Preview the previous or next profile for the current picture."],
        [["Space"], "Enable or disable the selected profile for the current picture."],
        [["Double-click"], "Enable only that profile thumbnail for the current picture."],
      ],
    ],
    [
      "Metadata",
      [
        [[","], "Focus tags."],
        [["/"], "Focus notes."],
        [["Enter"], "Save tags and advance; save notes and return to review."],
        [["Esc"], "Save tags or notes and return to review without advancing."],
      ],
    ],
    [
      "View",
      [
        [["f"], "Toggle fullscreen."],
        [["?", "Esc"], "Show or hide this shortcuts overlay."],
      ],
    ],
    ["Retouch", [[["Double-click"], "Double-click a retouch control name to reset that value."]]],
    [
      "Tools",
      [
        [["Crop", "OK"], "Open crop/rotate from Tools, adjust the frame, then apply it with OK."],
        [["Diffusion"], "Preview film-like softness and highlight glow for the selected profile."],
        [["r"], "Rotate the selected crop ratio while crop mode is open."],
      ],
    ],
  ];
  return (
    <Dialog
      id={"shortcuts-overlay"}
      className="shortcuts-overlay"
      labelledBy="shortcuts-title"
      label="Shortcuts"
      open={open}
      onClose={onClose}
    >
      <section class={"shortcuts-card"}>
        <header class={"shortcuts-header"}>
          <h2 id={"shortcuts-title"}>{"Shortcuts"}</h2>
          <button id={"shortcuts-close"} type={"button"} onClick={onClose}>
            {"Close"}
          </button>
        </header>
        <div class={"shortcut-sections"}>
          {sections.map(([title, rows]) => (
            <section key={title} class={"shortcut-section"}>
              <h3>{title}</h3>
              {rows.map(([keys, description]) => (
                <div key={`${title}-${description}`} class={"shortcut-row"}>
                  <span class={"shortcut-keys"}>
                    {keys.map((key) => (
                      <kbd key={key}>{key}</kbd>
                    ))}
                  </span>
                  <span>{description}</span>
                </div>
              ))}
            </section>
          ))}
        </div>
      </section>
    </Dialog>
  );
}
