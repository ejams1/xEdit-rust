{ parity probe: a-31 }
unit P_a_31;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..1] of Integer;
  b: array[0..1] of Integer;
begin
  a[0] := 3;
  b[0] := a[0];
  Show('b[0]', b[0]);
end;
end.
