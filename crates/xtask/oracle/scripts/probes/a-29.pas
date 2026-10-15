{ parity probe: a-29 }
unit P_a_29;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..2] of Integer;
begin
  a := 5;
  Show('a', a);
end;
end.
