{ parity probe: x-272 }
unit P_x_272;

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
  v := StrToInt(nil);
  AddMessage('after');
end;
end.
