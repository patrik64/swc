var _async_to_generator = require("@swc/helpers/_/_async_to_generator");
var _class_call_check = require("@swc/helpers/_/_class_call_check");
var _create_class = require("@swc/helpers/_/_create_class");
var _ts_generator = require("@swc/helpers/_/_ts_generator");
var foo = {
    bar: function bar(_0) {
        return /*#__PURE__*/ _async_to_generator._(function(param) {
            var name;
            var _arguments = arguments;
            return _ts_generator._(this, function(_state) {
                name = param.name;
                console.log("arguments.length", _arguments.length);
                return [
                    2
                ];
            });
        }).apply(this, arguments);
    }
};
var Foo = /*#__PURE__*/ function() {
    "use strict";
    function Foo() {
        _class_call_check._(this, Foo);
    }
    _create_class._(Foo, [
        {
            key: "bar",
            value: function bar(_0) {
                return /*#__PURE__*/ _async_to_generator._(function(param) {
                    var name;
                    var _arguments = arguments;
                    return _ts_generator._(this, function(_state) {
                        name = param.name;
                        console.log("arguments.length", _arguments.length);
                        return [
                            2
                        ];
                    });
                }).apply(this, arguments);
            }
        }
    ]);
    return Foo;
}();
function bar(_0) {
    return /*#__PURE__*/ _async_to_generator._(function(param) {
        var name;
        var _arguments = arguments;
        return _ts_generator._(this, function(_state) {
            name = param.name;
            console.log("arguments.length", _arguments.length);
            return [
                2
            ];
        });
    }).apply(this, arguments);
}
