import { mount } from "svelte";
import "./fonts.css";
import App from "./App.svelte";
import { motionProperties } from "./lib/motion";

// The springs are custom properties on the root, so every stylesheet eases on the same curves.
for (const [name, value] of Object.entries(motionProperties())) {
  document.documentElement.style.setProperty(name, value);
}

const target = document.getElementById("app");
if (!target) throw new Error("missing #app mount point in index.html");

mount(App, { target });
