"use strict";
Object.defineProperty(exports, "__esModule", {
    value: true
});
Object.defineProperty(exports, "default", {
    enumerable: true,
    get: function() {
        return Foo;
    }
});
const _async_to_generator = require("@swc/helpers/_/_async_to_generator");
const _interop_require_default = require("@swc/helpers/_/_interop_require_default");
const _react = /*#__PURE__*/ _interop_require_default._(require("react"));
function Foo() {
    return /*#__PURE__*/ _react.default.createElement("div", {
        onClick: (e)=>/*#__PURE__*/ /*#__PURE__*/ _async_to_generator._(function*() {
                yield doSomething();
            })()
    });
}
Foo.displayName = "Foo";
