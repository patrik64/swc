//// [test.ts]
System.register([
    "@swc/helpers/_/_async_to_generator"
], function(_export, _context) {
    "use strict";
    var _async_to_generator, cl1, cl2, obj, l;
    function fn() {
        return /*#__PURE__*/ _async_to_generator(function*() {
            const req = yield _context.import('./test') // ONE
            ;
        })();
    }
    _export({
        fn: fn,
        cl1: void 0,
        cl2: void 0
    });
    return {
        setters: [
            function(_async_to_generator1) {
                _async_to_generator = _async_to_generator1._;
            }
        ],
        execute: function() {
            _export("cl1", cl1 = class cl1 {
                m() {
                    return /*#__PURE__*/ _async_to_generator(function*() {
                        const req = yield _context.import('./test') // TWO
                        ;
                    })();
                }
            });
            _export("obj", obj = {
                m: ()=>/*#__PURE__*/ _async_to_generator(function*() {
                        const req = yield _context.import('./test') // THREE
                        ;
                    })()
            });
            _export("cl2", cl2 = class cl2 {
                constructor(){
                    this.p = {
                        m: ()=>/*#__PURE__*/ _async_to_generator(function*() {
                                const req = yield _context.import('./test') // FOUR
                                ;
                            })()
                    };
                }
            });
            _export("l", l = ()=>/*#__PURE__*/ _async_to_generator(function*() {
                    const req = yield _context.import('./test') // FIVE
                    ;
                })());
        }
    };
});
