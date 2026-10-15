{ parity probe: x-009 }
unit P_x_009;

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
  s := IntToStr(nil);
  AddMessage('after');
end;
end.
