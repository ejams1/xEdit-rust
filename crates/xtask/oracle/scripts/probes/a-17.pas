{ parity probe: a-17 }
unit P_a_17;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..1] of Double;
  b: array[0..1] of Boolean;
  s: array[0..1] of string;
begin
  Show('d', a[0]);
  Show('b', b[0]);
  Show('s', s[0]);
  Show('VarType d', VarType(a[0]));
  Show('VarType s', VarType(s[0]));
end;
end.
