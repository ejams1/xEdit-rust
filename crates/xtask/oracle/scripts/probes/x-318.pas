{ parity probe: x-318 }
unit P_x_318;

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
  sl.SaveToFile('');
  AddMessage('after');
end;
end.
