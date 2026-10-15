{ parity probe: x-332 }
unit P_x_332;

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
  v := TStringList is TStringList;
  AddMessage('after');
end;
end.
