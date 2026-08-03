import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { Resvg } from '@resvg/resvg-js'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
// 以 public/favicon.svg 为安装包图标源
const svg = readFileSync(join(root, 'public/favicon.svg'), 'utf8')
const resvg = new Resvg(svg, {
  fitTo: { mode: 'width', value: 1024 },
  background: 'rgba(0,0,0,0)',
})
const png = resvg.render().asPng()
const out = join(root, 'app-icon.png')
writeFileSync(out, png)
writeFileSync(join(root, 'app-icon.svg'), svg)
console.log('wrote', out, '(from public/favicon.svg)')
