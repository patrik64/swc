//// [asyncUnParenthesizedArrowFunction_es5.ts]
import { _ as _async_to_generator } from "@swc/helpers/_/_async_to_generator";
import { _ as _ts_generator } from "@swc/helpers/_/_ts_generator";
var x = function(i) {
    return /*#__PURE__*/ _async_to_generator(function() {
        return _ts_generator(this, function(_state) {
            switch(_state.label){
                case 0:
                    return [
                        4,
                        someOtherFunction(i)
                    ];
                case 1:
                    return [
                        2,
                        _state.sent()
                    ];
            }
        });
    })();
};
var x1 = function(i) {
    return /*#__PURE__*/ _async_to_generator(function() {
        return _ts_generator(this, function(_state) {
            switch(_state.label){
                case 0:
                    return [
                        4,
                        someOtherFunction(i)
                    ];
                case 1:
                    return [
                        2,
                        _state.sent()
                    ];
            }
        });
    })();
};
