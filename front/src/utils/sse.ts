interface SseEvent {
	id?: string;
	event: string;
	data?: string;
	retry?: string;
}

type ValidKeys = keyof SseEvent;
const VALID_KEYS = ["id", "event", "data", "retry"] as const;

function isValidKeys(key: string): key is ValidKeys {
	return VALID_KEYS.includes(key as ValidKeys);
}

/**
 * follow the spec: https://html.spec.whatwg.org/multipage/server-sent-events.html
 * @constructor
 */
export function SseParser(
	handleEvent: (ev: SseEvent) => void,
	endFn?: () => void
) {
	let isDropped = false;
	const lineQueue: string[] = [];
	let cacheData: Uint8Array = new Uint8Array();
	let isHanding = false;

	function parseSseEvent(lines: string[]): SseEvent | null {
		if (!lines.length) {
			return null;
		}
		const sseEvent: SseEvent = { event: "message" };
		for (const line of lines) {
			// If the line contains a U+003A COLON character (:) 
			const colonIndex = line.indexOf(":");
			if (colonIndex > -1) {
				const key = line.slice(0, colonIndex);
				if (isValidKeys(key)) {
					let value = line.slice(colonIndex + 1);
					// remove first u+0020
					if (value.startsWith("\u0020")) {
						value = value.slice(1);
					}
				sseEvent[key] = value;
				}
			} else {
				const key = line;
				if (isValidKeys(key)) {
					sseEvent[key] = "";
				}
			}
		}
		return sseEvent;
	}

	function handleCache(end = false) {
		if (isHanding) {
			throw new Error(
				"sse parser is broken, please report this fatal error to developer"
			);
		}
		isHanding = true;
		const messageLines: string[] = [];
		while (lineQueue.length) {
			const line = lineQueue.shift();
			if (line === undefined) {
				break;
			}
			// If the line is empty (a blank line)
			// Dispatch the event, as defined below.
			if (line === "") {
				const sseEvent = parseSseEvent(messageLines);
				if (sseEvent) {
					handleEvent(sseEvent);
				}
				messageLines.length = 0;
			}
			// If the line starts with a U+003A COLON character (:) 
			// Ignore the line.
			else if (line.startsWith(":")) {
				continue;
			}
			else {
                messageLines.push(line);
            }
		}
		// Put back any unprocessed lines
		lineQueue.unshift(...messageLines);
		if (end) {
            // If stream ends and there are still message lines, dispatch one last time
            if (messageLines.length > 0) {
                const sseEvent = parseSseEvent(messageLines);
                if (sseEvent) {
                    handleEvent(sseEvent);
                }
            }
			endFn?.();
		}
		isHanding = false;
	}

	function pushStream(value: Uint8Array) {
		if (isDropped) {
			return;
		}
		const total = new Uint8Array(cacheData.length + value.length);
		total.set(cacheData, 0);
		total.set(value, cacheData.length);

		const text = new TextDecoder().decode(total);
		const lines = text.split(/\r\n|\n|\r/);
		
        const lastLine = lines.pop() || '';

		lineQueue.push(...lines);
		cacheData = new TextEncoder().encode(lastLine);
		
		handleCache();
	}

	function endStream() {
		if (isDropped) {
			return;
		}
        const lastLine = new TextDecoder().decode(cacheData);
        if (lastLine) {
            lineQueue.push(lastLine);
            cacheData = new Uint8Array();
        }
		handleCache(true);
	}

	function dropStream() {
		isDropped = true;
		lineQueue.length = 0;
	}

	return {
		dropStream,
		pushStream,
		endStream
	};
}

export function parseSse(text: string): SseEvent[] {
	const events: SseEvent[] = [];

	const parser = SseParser((ev) => {
		events.push(ev);
	});

    // Add trailing newlines to ensure the last event is processed correctly,
    // as the parser dispatches events on blank lines.
	const textToParse = text.endsWith('\n\n') ? text : text + '\n\n';

	parser.pushStream(new TextEncoder().encode(textToParse));
	parser.endStream();

	return events;
}