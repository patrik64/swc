//// [foo.ts]
"use strict";
Object.defineProperty(exports, "__esModule", {
    value: true
});
Object.defineProperty(exports, "default", {
    enumerable: true,
    get: function() {
        return _default;
    }
});
const _default = "./foo";
//// [index.ts]
"use strict";
Object.defineProperty(exports, "__esModule", {
    value: true
});
const _async_to_generator = require("@swc/helpers/_/_async_to_generator");
const _interop_require_wildcard = require("@swc/helpers/_/_interop_require_wildcard");
function foo() {
    return /*#__PURE__*/ /*#__PURE__*/ _async_to_generator._(function*() {
        return yield Promise.resolve((yield Promise.resolve().then(()=>/*#__PURE__*/ _interop_require_wildcard._(require("./foo")))).default).then((p)=>/*#__PURE__*/ _interop_require_wildcard._(require(p)));
    })();
}
