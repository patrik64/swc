var _async_to_generator = require("@swc/helpers/_/_async_to_generator");
const obj = {
    // A comment
    foo () {
        return /*#__PURE__*/ _async_to_generator._(function*() {
            console.log("Should work");
        })();
    }
};
