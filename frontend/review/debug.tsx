/** Load Preact's development checks before the application; release builds enter main.tsx directly. */
import "preact/debug";
import "./main";
