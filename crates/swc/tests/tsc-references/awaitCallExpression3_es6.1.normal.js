//// [awaitCallExpression3_es6.ts]
import { _ as _async_to_generator } from "@swc/helpers/_/_async_to_generator";
function func() {
    return /*#__PURE__*/ _async_to_generator(function*() {
        before();
        var b = fn(a, (yield p), a);
        after();
    })();
}
