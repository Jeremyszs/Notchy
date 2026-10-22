import { mount } from "svelte";
import "../fonts.css";
import Settings from "./Settings.svelte";

const target = document.getElementById("app");
if (!target) throw new Error("missing #app mount point in settings.html");

mount(Settings, { target });
