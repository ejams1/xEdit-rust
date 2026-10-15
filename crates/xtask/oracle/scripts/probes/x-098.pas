{ parity probe: x-098 }
unit P_x_098;

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
  AddMessage(sl);
  AddMessage('after');
end;
end.
