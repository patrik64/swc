//// [awaitCallExpression4_es6.ts]
import { _ as _async_to_generator } from "@swc/helpers/_/_async_to_generator";
function func() {
    return /*#__PURE__*/ _async_to_generator(function*() {
        before();
        var b = (yield pfn)(a, a, a);
        after();
    })();
}
