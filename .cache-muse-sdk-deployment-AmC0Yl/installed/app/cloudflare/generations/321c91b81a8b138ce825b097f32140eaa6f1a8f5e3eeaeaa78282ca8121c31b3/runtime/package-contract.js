// Decode the compiler's package-host data section without loading provider
// codecs. Ordinary programs need this validation but never instantiate a
// package runtime.
import { HostContract } from "./wire.js";

                           
                                         
                        
                          
                         
                         
                                     
                                           
                
                               
                 
                                                    
                                   
                  
                  
                     
 
                                 
               
                 
                        
                          
                        
 
                                    
                             
                 
                      
                  
                            
 

const fail = (message        )        => {
  throw new HostContract(`package host: ${message}`);
};

function validSelector(item                   , selector        )          {
  const parts = selector.split(".");
  return (
    !item.handle && parts.length >= 2 && ["dep", "pkg"].includes(parts[0] ) && parts.every(Boolean)
  );
}

function validateContract(interfaces                     )       {
  const identities = new Set        ();
  const modules = new Set        ();
  const selectors = new Set        ();
  const unique = (set             , key        )       => {
    if (!key || set.has(key)) {
      fail("empty or duplicate contract name");
    }
    set.add(key);
  };
  const checkFields = (fields                 )       => {
    const names = new Set        ();
    for (const [name, value] of fields) {
      unique(names, name);
      checkValue(value);
    }
  };
  // The recursive types around the value being checked, outermost first,
  // and how many of them a list, option, result or enum of more than one
  // variant guards (hd_host_abi schema.rs `Binders`).
  const binders           = [];
  let guard = 0;
  const checkValue = (value               )       => {
    const outer = guard;
    if (
      [8, 9, 13, 18].includes(value[0]) ||
      (value[0] === 12 && (value[1]             ).length > 1)
    ) {
      guard = binders.length;
    }
    checkShape(value);
    guard = outer;
  };
  const checkShape = (value               )       => {
    const [tag, left, right] = value;
    // `never` is a method's whole result only (module.boundary.never-result).
    if (tag === 19) {
      fail("never outside a method's whole result");
    }
    if (tag === 15 && !interfaces.some((item) => item.handle && item.module === left)) {
      fail("missing handle interface");
    }
    if (tag === 8 || tag === 9) {
      checkValue(left);
    }
    if (tag === 10) {
      left.forEach(checkValue);
    }
    if (tag === 11) {
      checkFields(left);
    }
    if (tag === 12) {
      const names = new Set        ();
      for (const [name, fields] of left) {
        unique(names, name);
        checkFields(fields);
      }
    }
    if (tag === 13 || tag === 18) {
      checkValue(left);
      checkValue(right);
    }
    if (tag === 16 || tag === 17) {
      checkRecursive(value);
    }
  };
  // `[16, id, body]` binds `id` over a struct or enum body; `[17, id]`
  // names a binder around it, guarded since.
  const checkRecursive = (value               )       => {
    if (value[0] === 16) {
      const [, id, body] = value;
      if (binders.includes(id) || ![11, 12].includes(body[0])) {
        fail("invalid recursive type");
      }
      binders.push(id);
      checkValue(body);
      binders.pop();
    } else if (value[0] === 17) {
      const at = binders.lastIndexOf(value[1]);
      if (at < 0 || at >= guard) {
        fail("unbound or unguarded recursive type");
      }
    }
  };
  for (const item of interfaces) {
    if (
      !item.identity.package ||
      !item.identity.trait_path ||
      !item.module.startsWith("hd:pkg:") ||
      item.module.length === 7
    ) {
      fail("invalid nominal identity");
    }
    unique(identities, JSON.stringify([item.identity, item.handle]));
    unique(modules, item.module);
    for (const selector of item.selectors) {
      if (!validSelector(item, selector)) {
        fail("invalid source selector");
      }
      unique(selectors, selector);
    }
    const methods = new Set        ();
    const imports = new Set        (item.handle ? ["$release"] : []);
    for (const method of item.methods) {
      unique(methods, method.name);
      if (!method.member) {
        fail("empty method member");
      }
      for (const name of method.wait === "may"
        ? [`${method.name}.start`, `${method.name}.finish`]
        : [method.name]) {
        unique(imports, name);
      }
      method.params.forEach(checkValue);
      if (method.result[0] !== 19) {
        checkValue(method.result);
      }
    }
  }
}

export function readPackageContract(bytes            )                      {
  if (!(bytes instanceof Uint8Array)) {
    fail("contract is not bytes");
  }
  let at = 0;
  const byte = ()         => {
    if (at >= bytes.length) {
      fail("truncated contract");
    }
    return bytes[at++] ;
  };
  const boolean = ()          => {
    const byteValue = byte();
    if (byteValue > 1) {
      fail("invalid contract boolean");
    }
    return byteValue === 1;
  };
  // A canonical unsigned LEB128 number below 2^32.
  const number = ()         => {
    let count = 0;
    for (let shift = 0; shift < 35; shift += 7) {
      const byteValue = byte();
      if (shift === 28 && byteValue > 15) {
        fail("oversized contract length");
      }
      count += (byteValue & 127) * 2 ** shift;
      if (byteValue < 128) {
        if (shift && byteValue === 0) {
          fail("invalid contract length");
        }
        return count;
      }
    }
    return fail("invalid contract length");
  };
  // A length, which the remaining bytes must hold.
  const count = ()         => {
    const count = number();
    if (count > bytes.length - at) {
      fail("invalid contract length");
    }
    return count;
  };
  const string = ()         => {
    const length = count();
    const chunk = bytes.subarray(at, at + length);
    at += length;
    try {
      return new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(chunk);
    } catch {
      return fail("invalid contract UTF-8");
    }
  };
  const vector =       (read            )         => Array.from({ length: count() }, read);
  const fields = (depth        )                  =>
    vector               (() => [string(), value(depth + 1)]);
  const value = (depth = 0)                => {
    if (depth > 32) {
      fail("contract nesting exceeds the boundary limit");
    }
    const tag = byte();
    if ([0, 1, 2, 4, 5, 6, 7, 14, 19].includes(tag)) {
      return [tag                                       ];
    }
    if (tag === 3) {
      const bits = byte();
      const signed = boolean();
      if (![8, 16, 32, 64].includes(bits)) {
        fail("invalid integer width");
      }
      return [tag, bits, signed];
    }
    if ([8, 9].includes(tag)) {
      return [tag         , value(depth + 1)];
    }
    if (tag === 10) {
      return [tag, vector(() => value(depth + 1))];
    }
    if (tag === 11) {
      return [tag, fields(depth)];
    }
    if (tag === 12) {
      return [tag, vector                           (() => [string(), fields(depth)])];
    }
    if ([13, 18].includes(tag)) {
      return [tag           , value(depth + 1), value(depth + 1)];
    }
    if (tag === 15) {
      return [tag, string()];
    }
    if (tag === 16) {
      return [tag, number(), value(depth + 1)];
    }
    if (tag === 17) {
      return [tag, number()];
    }
    return fail("unknown contract value tag");
  };
  if ([byte(), byte(), byte(), byte()].join(",") !== "1,0,0,0") {
    fail("unsupported ABI version");
  }
  const interfaces = vector                   (() => ({
    identity: { package: string(), version: string(), trait_path: string() },
    module: string(),
    selectors: vector(string),
    handle: boolean(),
    methods: vector(() => ({
      name: string(),
      member: string(),
      wait: boolean() ? ("may"         ) : ("never"         ),
      params: vector(() => value()),
      result: value(),
    })),
  }));
  if (at !== bytes.length) {
    fail("trailing contract bytes");
  }
  validateContract(interfaces);
  return interfaces;
}
