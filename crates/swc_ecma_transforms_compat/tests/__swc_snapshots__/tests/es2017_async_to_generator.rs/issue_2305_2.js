function MyClass() {}
MyClass.prototype.handle = function() {
    console.log('this is MyClass handle');
};
MyClass.prototype.init = function(param1) {
    return /*#__PURE__*/ _async_to_generator(function() {
        var a;
        return _ts_generator(this, function(_state) {
            a = 1;
            if (!param1) {
                console.log(this);
                this.handle();
            }
            if (param1 === a) {
                return [
                    2,
                    false
                ];
            }
            return [
                2,
                true
            ];
        });
    }).call(this);
};
const myclass = new MyClass();
myclass.handle();
