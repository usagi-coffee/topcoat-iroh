import { connect, openApp } from "/__topcoat/connect.js";
import { connectionFragment } from "/__topcoat/fragment.js";
import { connectionSettings } from "/__topcoat/settings.js";
import { get } from "/__topcoat/storage.js";

const fragment = connectionFragment(location.href);
if (fragment.present) history.replaceState(history.state, "", fragment.href);

const custom = document.querySelector('[data-topcoat-launcher="custom"]');
try {
	const config = await fetch("/__topcoat/config.json", {
		cache: "no-store",
	}).then((response) => {
		if (!response.ok) throw new Error("Cannot load connection config");
		return response.json();
	});
	if (!custom || fragment.present) {
		const saved = (await get("connection")) ?? {
			endpoint: (await get("endpoint")) ?? "",
		};
		await connect(connectionSettings(fragment, config, saved));
		openApp();
	}
} catch (error) {
	const status =
		document.querySelector('[role="status"]') ??
		document.body.appendChild(document.createElement("p"));
	status.textContent = String(error);
}
