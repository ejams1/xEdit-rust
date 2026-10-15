{ parity probe: a-35 }
unit P_a_35;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..2, 0..2] of Integer;
begin
  SetLength(a, 5);
  AddMessage('after');
end;
end.
