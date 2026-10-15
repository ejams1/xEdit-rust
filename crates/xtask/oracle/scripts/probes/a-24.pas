{ parity probe: a-24 }
unit P_a_24;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..2] of string;
begin
  a[1] := 5;
  Show('a[1]', a[1]);
  Show('VarType', VarType(a[1]));
end;
end.
