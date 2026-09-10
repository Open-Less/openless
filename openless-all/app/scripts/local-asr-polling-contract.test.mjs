import { readFile } from 'node:fs/promises';
import ts from 'typescript';

const source = await readFile(new URL('../src/pages/LocalAsr/index.tsx', import.meta.url), 'utf-8');

const refreshPolling =
  source.match(/window\.setInterval\(\(\) => \{\s*void refresh\(\);?\s*\}, 3000\)/g) ?? [];

const sourceFile = ts.createSourceFile(
  'LocalAsr/index.tsx',
  source,
  ts.ScriptTarget.Latest,
  true,
  ts.ScriptKind.TSX,
);
const localAsr = sourceFile.statements.find(
  (statement) => ts.isFunctionDeclaration(statement) && statement.name?.text === 'LocalAsr',
);
if (!localAsr?.body) {
  throw new Error('LocalAsr component declaration is missing');
}

const jsxComponentNames = new Set();
const collectJsxComponentNames = (node) => {
  if (
    (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) &&
    ts.isIdentifier(node.tagName)
  ) {
    jsxComponentNames.add(node.tagName.text);
  }
  ts.forEachChild(node, collectJsxComponentNames);
};
collectJsxComponentNames(localAsr.body);

const createsComponentType = (node) =>
  ts.isArrowFunction(node) ||
  ts.isFunctionExpression(node) ||
  (ts.isConditionalExpression(node) &&
    (createsComponentType(node.whenTrue) || createsComponentType(node.whenFalse)));

const nestedJsxComponents = localAsr.body.statements.flatMap((statement) => {
  if (!ts.isVariableStatement(statement)) return [];
  return statement.declarationList.declarations
    .filter(
      (declaration) =>
        ts.isIdentifier(declaration.name) &&
        jsxComponentNames.has(declaration.name.text) &&
        declaration.initializer &&
        createsComponentType(declaration.initializer),
    )
    .map((declaration) => declaration.name.text);
});

if (nestedJsxComponents.length > 0) {
  throw new Error(
    `LocalAsr must not create JSX component types during render; found ${nestedJsxComponents.join(', ')}`,
  );
}

if (refreshPolling.length !== 1) {
  throw new Error(`LocalAsr should have one refresh poller, found ${refreshPolling.length}`);
}

if (
  !/if \(downloadDialogOpen\) return[\s\S]{0,200}window\.setInterval\(\(\) => \{\s*void refresh\(\)/.test(
    source,
  )
) {
  throw new Error('LocalAsr refresh polling must stop while the download dialog is open');
}

for (const contract of [
  'const downloadDialogOpenRef = useRef(downloadDialogOpen)',
  'const refreshGenerationRef = useRef(0)',
  'const makeRefreshGuard = (): RefreshGuard =>',
  'refreshGenerationRef.current += 1',
  'if (!isCurrent()) return',
]) {
  if (!source.includes(contract)) {
    throw new Error(`LocalAsr refresh guard contract is missing: ${contract}`);
  }
}

console.log(
  'LocalAsr keeps one refresh poller, pauses it for the download dialog, and preserves stable JSX component types',
);
