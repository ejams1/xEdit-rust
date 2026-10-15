{ parity probe: x-068 }
unit P_x_068;

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
  v := StrToInt('');
  AddMessage('after');
end;
end.
