{ parity probe: x-247 }
unit P_x_247;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  v := ElementCount(Unassigned); AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
