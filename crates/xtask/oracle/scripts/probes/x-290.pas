{ parity probe: x-290 }
unit P_x_290;

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
  sl.Add(Unassigned); AddMessage('[' + sl[1] + ']');
  AddMessage('after');
end;
end.
