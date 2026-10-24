import { mount } from "svelte";
import "../fonts.css";
import Settings from "./Settings.svelte";
import { motionProperties } from "../lib/motion";

for (const [name, value] of Object.entries(motionProperties())) {
  document.documentElement.style.setProperty(name, value);
}

const target = document.getElementById("app");
if (!target) throw new Error("missing #app mount point in settings.html");

mount(Settings, { target });
