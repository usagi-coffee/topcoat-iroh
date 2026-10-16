import { irohTransport } from "./render/iroh";
import { Runtime } from "./runtime";

const module = document.querySelector<HTMLScriptElement>(
	"script[data-topcoat-iroh]",
)?.dataset.topcoatIroh;
const runtime = new Runtime(module ? irohTransport(module) : undefined);
runtime.start(document);
runtime.page.listenForDevRefresh();
