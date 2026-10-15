{ parity probe: a-05 }
unit P_a_05;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array of IInterface;
begin
  SetLength(a, 2);
  Show('Assigned', Assigned(a[0]));
  Show('VarType', VarType(a[0]));
end;
end.
