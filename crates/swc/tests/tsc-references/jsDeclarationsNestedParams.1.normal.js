//// [file.js]
import { _ as _async_to_generator } from "@swc/helpers/_/_async_to_generator";
class X {
    /**
      * Cancels the request, sending a cancellation to the other party
      * @param {Object} error __auto_generated__
      * @param {string?} error.reason the error reason to send the cancellation with
      * @param {string?} error.code the error code to send the cancellation with
      * @returns {Promise.<*>} resolves when the event has been sent.
      */ cancel(_0) {
        return /*#__PURE__*/ _async_to_generator(function*({ reason, code }) {}).apply(this, arguments);
    }
}
class Y {
    /**
      * Cancels the request, sending a cancellation to the other party
      * @param {Object} error __auto_generated__
      * @param {string?} error.reason the error reason to send the cancellation with
      * @param {Object} error.suberr
      * @param {string?} error.suberr.reason the error reason to send the cancellation with
      * @param {string?} error.suberr.code the error code to send the cancellation with
      * @returns {Promise.<*>} resolves when the event has been sent.
      */ cancel(_0) {
        return /*#__PURE__*/ _async_to_generator(function*({ reason, suberr }) {}).apply(this, arguments);
    }
}
