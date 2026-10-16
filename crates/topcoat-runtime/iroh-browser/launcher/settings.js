export function connectionOptions({ endpoint = "", ticket = "", secret = "" }) {
	if ([endpoint, ticket, secret].some((value) => typeof value !== "string"))
		throw new Error("Connection fields must be strings");
	endpoint = endpoint.trim();
	ticket = ticket.trim();
	secret = secret.trim();
	if (!!endpoint === !!ticket)
		throw new Error("Supply either an endpoint or a ticket");
	if (endpoint.length > 8192 || ticket.length > 8192 || secret.length > 256)
		throw new Error("Connection field is too long");
	return { endpoint, ticket, secret };
}

// A link replaces the configured destination as a whole, keeping its fields distinct.
export function connectionSettings(fragment, config, saved) {
	const fromLink = fragment.endpoint !== null || fragment.ticket !== null;
	const source = fromLink
		? fragment
		: config.endpoint || config.ticket
			? config
			: saved;
	const endpoint = source.endpoint ?? "";
	const ticket = source.ticket ?? "";
	const secret =
		fragment.secret ??
		(!fromLink &&
		endpoint === (saved.endpoint ?? "") &&
		ticket === (saved.ticket ?? "")
			? (saved.secret ?? "")
			: "");
	return { endpoint, ticket, secret };
}
