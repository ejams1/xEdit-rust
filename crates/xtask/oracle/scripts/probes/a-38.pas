{ parity probe: a-38 }
unit P_a_38;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..2] of Integer;
  i: Integer;
begin
  i := 3;
  a[i] := 1;
  AddMessage('after');
end;
end.
