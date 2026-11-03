// Does casement show the same words as the web render of the same IntUI tree?
// Usage: bun rig/web-compare.ts <intui-tree.json> <casement-/tree.json>
// Draws the tree with @intui/web in happy-dom, reads what a person would read (no screen-reader-only
// or aria-hidden text) and the pressable buttons, reads the same from casement's /tree, and compares
// words and buttons in order. Prints one line per kind of difference; exits 1 if there is any.
import { readFileSync } from 'node:fs'

// IntUI is a sibling checkout; its modules are loaded by path so this file can live here.
const intui = process.env.INTUI ?? `${process.env.HOME}/dev/projects/intui`
// happy-dom has no CSS.escape, which the web Ladder uses to find its nodes; ids here are plain, so identity does.
;(globalThis as any).CSS ??= { escape: (s: string) => s }
const { Window } = await import(`${intui}/packages/web/node_modules/happy-dom`)
const { Tree } = await import(`${intui}/packages/core/src/index.ts`)
const { render } = await import(`${intui}/packages/web/src/index.ts`)

const [treePath, casementPath] = process.argv.slice(2)
if (!treePath || !casementPath) throw new Error('usage: web-compare.ts <intui-tree.json> <casement-tree.json>')

const window = new Window()
const document = window.document as unknown as Document
const parsed = Tree.parse(JSON.parse(readFileSync(treePath, 'utf8')))
if (parsed.kind !== 'parsed') throw new Error(parsed.message)
const tree = parsed.value
const root = render(tree, { source: 'compare', document, theme: 'light', copyButton: false })
document.body.append(root)

const words = (s: string) => s.replace(/[|*_`#•·~≥\[\]]/g, ' ').split(/\s+/).filter(Boolean)

/** What the page shows, in order: text nodes outside hidden or screen-reader-only elements. */
function webWords(node: Node): string[] {
  if (node.nodeType === 3) return words(node.textContent ?? '')
  const el = node as Element
  // A reading's mark (~, a dash) is hidden from screen readers but is on show, so it counts.
  const onShow = el.classList?.contains('intui-reading-mark') || el.classList?.contains('intui-reading-none')
  if (el.classList?.contains('intui-sr') || (el.getAttribute?.('aria-hidden') === 'true' && !onShow) || el.hasAttribute?.('hidden')) return []
  // A <select> is one control; its options are not on show until it opens (native shows the chosen one in a field).
  if (el.tagName?.toLowerCase() === 'select') return []
  // An SVG <title> is a tooltip, shown only on hover.
  if (el.tagName?.toLowerCase() === 'title') return []
  // A closed <details> ("Show as text") holds the text form for people who ask for it; it is not on show.
  if (el.tagName === 'DETAILS' && !el.hasAttribute('open')) return []
  return [...node.childNodes].flatMap(webWords)
}
const webButtons = [...root.querySelectorAll('button')].map(b => words(b.textContent ?? '').join(' '))

type Casement = { kind: string; text?: string; children?: Casement[] }
const casement: Casement = JSON.parse(readFileSync(casementPath, 'utf8'))
const walk = (n: Casement): Casement[] => [n, ...(n.children ?? []).flatMap(walk)]
const nodes = walk(casement)
const casementWords = nodes.filter(n => n.kind === 'label' || n.kind === 'button').flatMap(n => words(n.text ?? ''))
const casementButtons = nodes.filter(n => n.kind === 'button').map(n => words(n.text ?? '').join(' '))

if (process.env.DUMP) {
  console.log('web:   ', webWords(root).join(' '))
  console.log('native:', casementWords.join(' '))
}
let differences = 0
const compare = (what: string, web: string[], native: string[]) => {
  const at = web.findIndex((w, i) => w !== native[i])
  if (at === -1 && web.length === native.length) return console.log(`${what}: same (${web.length})`)
  differences++
  console.log(`${what}: DIFFER at ${at === -1 ? Math.min(web.length, native.length) : at}`)
  console.log(`  web:    ${web.slice(Math.max(0, at - 2), at + 6).join(' ')}`)
  console.log(`  native: ${native.slice(Math.max(0, at - 2), at + 6).join(' ')}`)
}
if (process.env.ANYORDER) {
  // A Ladder's DOM lists its rungs and steps in an order that is not the order they are drawn in,
  // so for trees with one the words are compared as sorted lists.
  const sorted = (a: string[]) => [...a].sort()
  compare('words, any order', sorted(webWords(root)), sorted(casementWords))
} else compare('words in order', webWords(root), casementWords)
compare('buttons', webButtons, casementButtons)
process.exit(differences ? 1 : 0)
