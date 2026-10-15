{ parity probe: a-12 }
unit P_a_12;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..2] of Integer;
begin
  SetLength(a, 5);
  a[4] := 9;
  Show('a[4]', a[4]);
  Show('High', High(a));
end;
end.
