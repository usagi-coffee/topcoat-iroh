// Connection fields belong to the bootstrap; other fragment fields belong to the app.
export function connectionFragment(href) {
	const url = new URL(href);
	const fields = new URLSearchParams(url.hash.slice(1));
	const reserved = ["endpoint", "ticket", "secret", "iroh"];
	const present = reserved.some((name) => fields.has(name));
	const endpoint = fields.get("endpoint") ?? fields.get("iroh");
	const ticket = fields.get("ticket");
	const secret = fields.get("secret");
	if (present) {
		for (const name of reserved) fields.delete(name);
		url.hash = fields.toString();
	}
	return { present, endpoint, ticket, secret, href: url.href };
}
