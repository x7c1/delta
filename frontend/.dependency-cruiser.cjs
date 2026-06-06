/**
 * dependency-cruiser configuration enforcing the package layer dependency
 * direction. All arrows point toward @delta/model:
 *
 *   web        -> ui-kit, api-client, model (+ dev: api-mocks)
 *   api-client -> model
 *   api-mocks  -> model
 *   ui-kit     -> (nothing domain/API)
 *   model      -> (nothing)
 *
 * Packages are matched by their source path under packages/<category>/<name>.
 */

/** Build a forbidden rule: sources under `fromDir` must not reach `toDir`. */
function forbid(name, comment, fromPath, toPath) {
  return {
    name,
    comment,
    severity: 'error',
    from: { path: fromPath },
    to: { path: toPath },
  };
}

const MODEL = 'packages/domain/model/';
const UI_KIT = 'packages/ui/ui-kit/';
const API_CLIENT = 'packages/gateway/api-client/';
const API_MOCKS = 'packages/testing/api-mocks/';
const WEB = 'packages/apps/web/';

module.exports = {
  forbidden: [
    forbid(
      'model-depends-on-nothing',
      'domain/model is pure: it must not depend on any other workspace package.',
      `^${MODEL}`,
      `^(${UI_KIT}|${API_CLIENT}|${API_MOCKS}|${WEB})`,
    ),
    forbid(
      'ui-kit-stays-generic',
      'ui-kit is domain-agnostic: no dependencies on model, API, mocks, or web.',
      `^${UI_KIT}`,
      `^(${MODEL}|${API_CLIENT}|${API_MOCKS}|${WEB})`,
    ),
    forbid(
      'api-client-only-model',
      'api-client may depend on model only (not ui-kit, mocks, or web).',
      `^${API_CLIENT}`,
      `^(${UI_KIT}|${API_MOCKS}|${WEB})`,
    ),
    forbid(
      'api-mocks-only-model',
      'api-mocks may depend on model only (not ui-kit, api-client, or web).',
      `^${API_MOCKS}`,
      `^(${UI_KIT}|${API_CLIENT}|${WEB})`,
    ),
    forbid(
      'nothing-depends-on-web',
      'web is the application root: no other package may depend on it.',
      `^(${MODEL}|${UI_KIT}|${API_CLIENT}|${API_MOCKS})`,
      `^${WEB}`,
    ),
    {
      name: 'no-circular',
      comment:
        'Runtime circular dependencies are forbidden. Type-only cycles are ' +
        'allowed: they are erased at compile time and let an id type alias ' +
        'live alongside the model it identifies even when two models ' +
        'reference each other (e.g. Thread <-> Message).',
      severity: 'error',
      from: {},
      to: { circular: true, dependencyTypesNot: ['type-only'] },
    },
  ],
  options: {
    doNotFollow: { path: 'node_modules' },
    exclude: { path: '(node_modules|dist|dist-types|\\.test\\.(ts|tsx)$)' },
    // Use a tsconfig whose `paths` map `@delta/*` to their TypeScript sources
    // rather than their built `dist` (which `exclude` would drop), so the
    // cross-package edges are recorded and the layer rules are enforced.
    tsConfig: { fileName: 'tsconfig.depcruise.json' },
    tsPreCompilationDeps: true,
  },
};
